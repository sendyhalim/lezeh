use std::collections::HashMap;
use std::collections::VecDeque;
use std::process::Command;
use std::process::Stdio;

use ghub::v3::client::GithubClient;
use ghub::v3::pull_request as github_pull_request;
use phab_lib::client::config::CertIdentityConfig;
use phab_lib::client::config::PhabricatorClientConfig;
use phab_lib::client::phabricator::PhabricatorClient;
use phab_lib::dto::Task;
use phab_lib::dto::User;
use serde_json::Value;

use crate::config::Config;
use crate::config::RepositoryConfig;
use crate::types::ResultDynError;

pub struct DeploymentClient {
  pub config: Config,
  phabricator: PhabricatorClient,
  ghub: GithubClient,
}

impl DeploymentClient {
  pub fn new(config: Config) -> ResultDynError<DeploymentClient> {
    let cert_identity_config = CertIdentityConfig {
      pkcs12_path: config.phab.pkcs12_path.clone(),
      pkcs12_password: config.phab.pkcs12_password.clone(),
    };

    let phabricator = PhabricatorClient::new(PhabricatorClientConfig {
      host: config.phab.host.clone(),
      api_token: config.phab.api_token.clone(),
      cert_identity_config: Some(cert_identity_config),
    })?;

    let ghub = GithubClient::new(&config.ghub.api_token)?;

    return Ok(DeploymentClient {
      ghub,
      config,
      phabricator,
    });
  }
}

#[derive(Debug)]
pub struct MergeAllOutput {
  pub repo_path: String,
  pub tasks_in_master_branch: Vec<TaskInMasterBranch>,
  pub matched_task_branch_mappings: Vec<MatchedTaskBranchMapping>,
}

#[derive(Debug)]
pub struct MergeAllReposOutput {
  pub merge_all_outputs: Vec<MergeAllOutput>,
  pub task_by_id: HashMap<String, Task>,
  pub not_found_user_task_mappings: Vec<UserTaskMapping>,
}

#[derive(Debug)]
pub struct UserTaskMapping(pub User, pub Task);

#[derive(Debug)]
pub struct MatchedTaskBranchMapping(pub String, pub String);

#[derive(Debug)]
pub struct TaskInMasterBranch {
  pub commit_message: String,
  pub task_id: String,
}

impl DeploymentClient {
  pub async fn merge_all_repos(&self, task_ids: &Vec<&str>) -> ResultDynError<MergeAllReposOutput> {
    let tasks: Vec<Task> = self.phabricator.get_tasks_by_ids(task_ids.clone()).await?;
    let task_by_id: HashMap<String, Task> = tasks
      .iter()
      .map(|task| {
        return (task.id.clone(), task.clone());
      })
      .collect();

    let task_assignee_ids: Vec<&str> = tasks
      .iter()
      // For simplicity's sake, we can be sure that every task should
      // have been assigned to an engineer.
      .map(|task| task.assigned_phid.as_ref().unwrap().as_ref())
      .collect();

    let task_assignees: Vec<User> = self
      .phabricator
      .get_users_by_phids(task_assignee_ids.iter().map(AsRef::as_ref).collect())
      .await?;

    let task_assignee_by_phid: HashMap<String, User> = task_assignees
      .into_iter()
      .map(|user| (user.phid.clone(), user))
      .collect();

    let mut merge_results: Vec<ResultDynError<MergeAllOutput>> = vec![];

    for repo_config in self.config.deployment.repositories.iter() {
      let merge_result = self.merge_for_repo(repo_config, task_ids).await;
      merge_results.push(merge_result);
    }

    // Make sure that all is well
    let merge_results: Result<Vec<MergeAllOutput>, _> = merge_results.into_iter().collect();
    let merge_results = merge_results?;
    let not_found_user_task_mappings =
      TaskUtil::find_not_found_tasks(&merge_results, &task_by_id, &task_assignee_by_phid);

    return Ok(MergeAllReposOutput {
      merge_all_outputs: merge_results,
      not_found_user_task_mappings,
      task_by_id,
    });
  }

  pub async fn merge_for_repo(
    &self,
    repo_config: &RepositoryConfig,
    task_ids: &Vec<&str>,
  ) -> ResultDynError<MergeAllOutput> {
    // TODO: This doesnt work
    // println!("[Run] cd {}", repo_config.path);
    // exec(
    // &format!("cd /home/cermati/development/athena/midas"),
    // "Cannot git pull origin master",
    // )?;

    println!("[Run] git pull origin master");
    exec("git pull origin master", "Cannot git pull origin master")?;

    // This will sync deleted branch remotely, sometimes we've deleted remote branch
    // but it still appears locally under origin/<branchname> when running `git branch -r`.
    println!("[Run] git remote prune origin");
    exec("git remote prune origin", "Cannot git remote prune origin")?;

    println!("[Run] git fetch --all");
    exec("git fetch --all", "Cannot git fetch remote")?;

    println!("[Run] git branch -r");
    let remote_branches = exec("git branch -r", "Cannot get all remote branches")?;

    let filtered_branch_mappings: Vec<MatchedTaskBranchMapping> =
      TaskUtil::create_matching_task_and_branch(task_ids, &remote_branches.split('\n').collect());
    let tasks_in_master_branch = self.tasks_in_master_branch(&task_ids).await?;

    for MatchedTaskBranchMapping(_task_id, remote_branch) in filtered_branch_mappings.iter() {
      self.merge(repo_config, &remote_branch).await?;
    }

    return Ok(MergeAllOutput {
      tasks_in_master_branch,
      matched_task_branch_mappings: filtered_branch_mappings,
      repo_path: String::from(&repo_config.path),
    });
  }

  fn get_pull_request_title(&self, remote_branch: &str) -> ResultDynError<String> {
    let commit_messages = exec(
      &format!(
        "git log --oneline --pretty=format:%s master..{}",
        remote_branch
      ),
      "Cannot print out git log to get pull request title",
    )?;

    return commit_messages
      .split('\n')
      .last()
      .ok_or(failure::format_err!(
        "Failed to get pull request title from remote branch {}",
        remote_branch
      ))
      .map(|pr_title| pr_title.to_owned());
  }

  async fn merge(&self, repo_config: &RepositoryConfig, remote_branch: &str) -> ResultDynError<()> {
    // Create PR
    // -----------------------
    let branch_name = remote_branch.split('/').last().ok_or(failure::format_err!(
      "Could not get branch name from {}",
      remote_branch
    ))?;

    let input = github_pull_request::CreatePullRequestInput {
      title: &self.get_pull_request_title(remote_branch)?,
      repo_path: &repo_config.github_path,
      branch_name,
      into_branch: "master",
    };

    println!("[{}] Creating PR {:?}...", remote_branch, input);
    let res_body: Value = self.ghub.pull_request.create(input).await?;
    println!("[{}] Done creating PR", remote_branch);
    log::debug!("Response body {:?}", res_body);

    let pull_number: &str = &format!("{}", res_body["number"]);

    // Merge
    // -----------------------
    let merge_method = github_pull_request::GithubMergeMethod::Squash;
    let input = github_pull_request::MergePullRequestInput {
      repo_path: &repo_config.github_path,
      pull_number,
      merge_method,
    };
    println!("[{}] Merging PR {:?}...", remote_branch, input);
    let res_body: Value = self.ghub.pull_request.merge(input).await?;
    println!("[{}] Done merging PR", remote_branch);
    log::debug!("Response body {:?}", res_body);

    let merge_succeeded: bool = res_body["merged"].as_bool().ok_or(failure::err_msg(
      "Failed to parse merge pull request 'merged' to bool",
    ))?;

    if !merge_succeeded {
      return Err(failure::format_err!(
        "Failed to merge pull request {}",
        remote_branch
      ));
    }

    return Ok(());
  }

  async fn tasks_in_master_branch(
    &self,
    task_ids: &Vec<&str>,
  ) -> ResultDynError<Vec<TaskInMasterBranch>> {
    let git_log_handle = spawn_command_from_str("git log --oneline", None, Some(Stdio::piped()))?;

    let grep_regex_input = task_ids.iter().fold("".to_owned(), |acc, task_id| {
      if acc.is_empty() {
        return String::from(*task_id);
      }

      return format!("{}\\|{}", acc, task_id);
    });

    let grep_output = spawn_command_from_str(
      &format!("grep {}", grep_regex_input),
      Some(git_log_handle.stdout.unwrap().into()),
      None,
    )?
    .wait_with_output()?;

    let grep_output = handle_command_output(grep_output)?;
    let commit_messages: Vec<&str> = grep_output.lines().collect();

    return Ok(
      task_ids
        .iter()
        .flat_map(|task_id| -> Vec<(String, String)> {
          return commit_messages
            .iter()
            .filter_map(|commit_message: &&str| -> Option<(String, String)> {
              if !commit_message.contains(task_id) {
                return None;
              }

              return Some((String::from(*task_id), String::from(*commit_message)));
            })
            .collect();
        })
        .map(|(task_id, commit_message)| TaskInMasterBranch {
          task_id,
          commit_message,
        })
        .collect(),
    );
  }
}

fn spawn_command_from_str(
  command_str: &str,
  stdin: Option<Stdio>,
  stdout: Option<Stdio>,
) -> ResultDynError<std::process::Child> {
  let mut command_parts = command_str.split(' ').collect::<VecDeque<&str>>();

  let command = command_parts
    .pop_front()
    .ok_or(format!("Invalid command: {}", command_str))
    .map_err(failure::err_msg)?;

  let handle = Command::new(command)
    .args(command_parts)
    .stdin(stdin.unwrap_or(Stdio::null()))
    .stdout(stdout.unwrap_or(Stdio::piped()))
    .spawn()?;

  return Ok(handle);
}

fn vec_to_string(v: Vec<u8>) -> ResultDynError<String> {
  return std::str::from_utf8(&v)
    .map(String::from)
    .map_err(failure::err_msg);
}

fn stderr_to_err(stderr: Vec<u8>) -> ResultDynError<String> {
  let output_err = vec_to_string(stderr)?;

  return Err(failure::err_msg(output_err));
}

fn handle_command_output(output: std::process::Output) -> ResultDynError<String> {
  if !output.stderr.is_empty() {
    // Convert explicitly to Err.
    return stderr_to_err(output.stderr);
  }

  return vec_to_string(output.stdout);
}

fn exec(command_str: &str, _assertion_txt: &str) -> ResultDynError<String> {
  let command_result = spawn_command_from_str(command_str, None, None)?.wait_with_output()?;

  if !command_result.stderr.is_empty() {
    return stderr_to_err(command_result.stderr);
  }

  return vec_to_string(command_result.stdout);
}

// TODO: Move to another module
struct TaskUtil;

impl TaskUtil {
  fn create_matching_task_and_branch(
    task_ids: &Vec<&str>,
    remote_branches: &Vec<&str>,
  ) -> Vec<MatchedTaskBranchMapping> {
    return remote_branches
      .iter()
      .flat_map(|remote_branch| {
        let remote_branch = remote_branch.trim().to_owned();

        return task_ids
          .into_iter()
          .map(|task_id| {
            return MatchedTaskBranchMapping(
              String::from(task_id.to_owned()),
              remote_branch.clone(),
            );
          })
          .collect::<Vec<MatchedTaskBranchMapping>>();
      })
      .filter(|MatchedTaskBranchMapping(task_id, remote_branch)| {
        return remote_branch.contains(&task_id[..]);
      })
      .collect();
  }

  fn find_not_found_tasks(
    merge_results: &Vec<MergeAllOutput>,
    task_by_id: &HashMap<String, Task>,
    task_assignee_by_phid: &HashMap<String, User>,
  ) -> Vec<UserTaskMapping> {
    // Start filtering all the not found tasks
    let mut found_task_count_by_id: HashMap<String, usize> = task_by_id
      .values()
      .into_iter()
      .map(|task| {
        return (task.id.clone(), 0);
      })
      .collect();

    merge_results.iter().for_each(|merge_result| {
      for MatchedTaskBranchMapping(task_id, _remote_branch) in
        merge_result.matched_task_branch_mappings.iter()
      {
        let current_counter = found_task_count_by_id
          .get_mut(PhabricatorClient::clean_id(task_id))
          .unwrap();

        *current_counter += 1;
      }
    });

    let not_found_user_task_mappings: Vec<UserTaskMapping> = found_task_count_by_id
      // .keys()
      .into_iter()
      .filter(|(_task_id, count)| {
        return *count == 0 as usize;
      })
      .map(|(task_id, _count)| {
        let task = task_by_id.get(&task_id).unwrap();
        let user_id: String = task.assigned_phid.clone().unwrap();
        let user = task_assignee_by_phid.get(&user_id).unwrap();

        return UserTaskMapping(user.clone(), task.clone());
      })
      .collect();

    return not_found_user_task_mappings;
  }
}

#[cfg(test)]
mod test {
  use super::*;

  mod find_not_found_tasks {
    use super::*;
    use fake::Fake;
    use fake::Faker;

    #[test]
    fn it_should_return_not_found_tasks() {
      let mut task_1: Task = Faker.fake();
      task_1.id = "1234".into();
      task_1.assigned_phid = Some("haha".into());

      let mut task_2: Task = Faker.fake();
      task_2.id = "3333".into();
      task_2.assigned_phid = Some("wut".into());

      let task_by_id: HashMap<String, Task> = vec![task_1.clone(), task_2.clone()]
        .iter()
        .map(|task| {
          return (task.id.clone(), task.clone());
        })
        .collect();

      let mut user_1: User = Faker.fake();
      user_1.phid = task_1.assigned_phid.unwrap().clone();

      let mut user_2: User = Faker.fake();
      user_2.phid = "wut".into();

      let task_assignee_by_phid: HashMap<String, User> = vec![user_1, user_2]
        .iter()
        .map(|user| {
          return (user.phid.clone(), user.clone());
        })
        .collect();

      let merge_results: Vec<MergeAllOutput> = vec![MergeAllOutput {
        repo_path: String::from("/foo"),
        tasks_in_master_branch: vec![],
        matched_task_branch_mappings: vec![MatchedTaskBranchMapping(
          "3333".into(),
          "origin/bar_T3333_foo".into(),
        )],
      }];

      let not_found_user_task_mappings =
        TaskUtil::find_not_found_tasks(&merge_results, &task_by_id, &task_assignee_by_phid);

      assert_eq!(1, not_found_user_task_mappings.len());
    }
  }

  mod create_matching_task_and_branch {
    use super::*;

    #[test]
    fn it_should_create_matching_branch() {
      let matched_task_branch_mappings = TaskUtil::create_matching_task_and_branch(
        &vec!["1234", "444"],
        &vec!["hmm_123", "hey1234", "445"],
      );

      let expected_mappings = vec![MatchedTaskBranchMapping("1234".into(), "hey1234".into())];

      assert_eq!(1, matched_task_branch_mappings.len());

      for i in 0..expected_mappings.len() {
        let expected_mapping = expected_mappings.get(i).unwrap();
        let result_mapping = matched_task_branch_mappings.get(i).unwrap();

        assert_eq!(expected_mapping.0, result_mapping.0);
        assert_eq!(expected_mapping.1, result_mapping.1);
      }
    }
  }
}

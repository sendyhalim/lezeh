use std::collections::HashMap;
use std::collections::VecDeque;
use std::process::Command;
use std::process::Stdio;

use futures::future::FutureExt;
use futures::Future;
use phab_lib::client::phabricator::CertIdentityConfig;
use phab_lib::client::phabricator::PhabricatorClient;
use phab_lib::dto::Task;
use phab_lib::dto::User;

use crate::config::Config;
use crate::types::ResultDynError;

pub struct DeploymentClient {
  pub config: Config,
  phabricator: PhabricatorClient,
}

impl DeploymentClient {
  pub fn new(config: Config) -> ResultDynError<DeploymentClient> {
    let cert_identity_config = CertIdentityConfig {
      pkcs12_path: config.phab.pkcs12_path.as_ref(),
      pkcs12_password: config.phab.pkcs12_password.as_ref(),
    };

    let phabricator = PhabricatorClient::new(
      &config.phab.host,
      &config.phab.api_token,
      Some(cert_identity_config),
    )?;

    return Ok(DeploymentClient {
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
        return (task.phid.clone(), task.clone());
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

    let futs: Vec<
      std::pin::Pin<Box<dyn Future<Output = ResultDynError<MergeAllOutput>> + std::marker::Send>>,
    > = self
      .config
      .deployment
      .repositories
      .iter()
      .map(|repo_config| return self.merge_for_repo(&repo_config.path, task_ids).boxed())
      .collect();

    let merge_results = futures::future::join_all(futs).await;

    // Make sure that all is well
    let merge_results: Result<Vec<MergeAllOutput>, _> = merge_results.into_iter().collect();
    let merge_results = merge_results?;

    // Start filtering all the not found tasks
    let mut found_task_count_by_id: HashMap<String, usize> = tasks
      .iter()
      .map(|task| {
        return (task.phid.clone(), 0);
      })
      .collect();

    merge_results.iter().for_each(|merge_result| {
      for MatchedTaskBranchMapping(task_id, _remote_branch) in
        merge_result.matched_task_branch_mappings.iter()
      {
        let current_counter = found_task_count_by_id.entry(task_id.clone()).or_insert(0);

        *current_counter += 1;
      }
    });

    let not_found_user_task_mappings: Vec<UserTaskMapping> = found_task_count_by_id
      // .keys()
      .into_iter()
      .filter(|(task_id, count)| {
        println!("{} ---> {}", task_id, count);
        return *count == 0 as usize;
      })
      .map(|(task_id, _count)| {
        let task = task_by_id.get(&task_id).unwrap();
        let user_id: String = task.assigned_phid.clone().unwrap();
        let user = task_assignee_by_phid.get(&user_id).unwrap();

        return UserTaskMapping(user.clone(), task.clone());
      })
      .collect();

    return Ok(MergeAllReposOutput {
      merge_all_outputs: merge_results,
      not_found_user_task_mappings,
      task_by_id,
    });
  }

  pub async fn merge_for_repo(
    &self,
    repo_path: &str,
    task_ids: &Vec<&str>,
  ) -> ResultDynError<MergeAllOutput> {
    println!("[Run] cd {}", repo_path);
    exec(
      &format!("cd {}", repo_path),
      "Cannot git pull origin master",
    )?;

    println!("[Run] git pull origin master");
    exec("git pull origin master", "Cannot git pull origin master")?;

    println!("[Run] git fetch --all");
    exec("git fetch --all", "Cannot git fetch remote")?;

    println!("[Run] git branch -r");
    let remote_branches = exec("git branch -r", "Cannot get all remote branches")?;

    let filtered_branch_mappings: Vec<MatchedTaskBranchMapping> = remote_branches
      .split('\n')
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

    let tasks_in_master_branch = self.tasks_in_master_branch(&task_ids).await?;

    for MatchedTaskBranchMapping(_task_id, remote_branch) in filtered_branch_mappings.iter() {
      self.merge(&remote_branch)?;
    }

    return Ok(MergeAllOutput {
      tasks_in_master_branch,
      matched_task_branch_mappings: filtered_branch_mappings,
      repo_path: String::from(repo_path),
    });
  }

  fn merge(&self, remote_branch: &str) -> ResultDynError<()> {
    let splitted = remote_branch
      .split('/')
      .map(String::from)
      .collect::<Vec<String>>();

    let local_branch = splitted.get(1).unwrap();

    println!("[{}] Merging...", remote_branch);

    let namespace = format!("[{}]", local_branch);

    println!("{} git checkout {}", namespace, local_branch);
    exec(
      &format!("git checkout {}", local_branch),
      &format!("{} Cannot checkout", namespace),
    )?;

    println!("{} git rebase master", namespace);
    exec(
      "git rebase master",
      &format!("{} Cannot rebase master", namespace),
    )?;

    println!("{} git checkout master", namespace);
    exec(
      "git checkout master",
      &format!("{} Cannot checkout master", namespace),
    )?;

    println!("{} git merge {} --ff-only", namespace, local_branch);
    exec(
      &format!("git merge {} --ff-only", local_branch),
      &format!("{} Cannot checkout master", namespace),
    )?;

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

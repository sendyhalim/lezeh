use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;

use failure::Fail;
use futures::FutureExt;
use ghub::v3::branch::DeleteBranchInput;
use ghub::v3::client::GithubClient;
use ghub::v3::pull_request as github_pull_request;
use ghub::v3::pull_request::GithubMergeMethod;
use phab_lib::client::config::CertIdentityConfig;
use phab_lib::client::config::PhabricatorClientConfig;
use phab_lib::client::phabricator::PhabricatorClient;
use phab_lib::dto::Task;
use phab_lib::dto::User;
use serde_json::Value;
use slog::Logger;

use crate::command;
use crate::command::PresetCommand;
use crate::config::Config;
use crate::config::RepositoryConfig;
use crate::types::ResultDynError;

pub struct GlobalDeploymentClient {
  pub config: Config,
  phabricator: Arc<PhabricatorClient>,
  ghub: Arc<GithubClient>,
  repository_deployment_client_by_key: HashMap<String, RepositoryDeploymentClient>,
  logger: Logger,
}

impl GlobalDeploymentClient {
  pub fn new(config: Config, logger: Logger) -> ResultDynError<GlobalDeploymentClient> {
    let cert_identity_config = CertIdentityConfig {
      pkcs12_path: config.phab.pkcs12_path.clone(),
      pkcs12_password: config.phab.pkcs12_password.clone(),
    };

    let phabricator = Arc::new(PhabricatorClient::new(PhabricatorClientConfig {
      host: config.phab.host.clone(),
      api_token: config.phab.api_token.clone(),
      cert_identity_config: Some(cert_identity_config),
    })?);

    let ghub = Arc::new(GithubClient::new(&config.ghub.api_token)?);

    let repository_deployment_client_by_key: HashMap<String, RepositoryDeploymentClient> = config
      .deployment
      .repositories
      .clone()
      .into_iter()
      .map(|repo_config| {
        let repo_key = repo_config.clone().key;
        return (
          repo_key.clone(),
          RepositoryDeploymentClient::new(
            repo_config.clone(),
            ghub.clone(),
            logger.new(slog::o!("repo" => repo_key)),
          ),
        );
      })
      .collect();

    return Ok(GlobalDeploymentClient {
      ghub,
      config,
      phabricator,
      repository_deployment_client_by_key,
      logger,
    });
  }
}

#[derive(Debug, Clone, Fail)]
pub enum ClientOperationError {
  #[fail(display = "Merge failed, please see {}", pull_request_url)]
  MergeError {
    remote_branch: String,
    pull_request_url: String,
  },
}

#[derive(Debug)]
pub struct SuccesfulMergeOutput {
  pub remote_branch: String,
  pub pull_request_url: String,
}

#[derive(Debug)]
pub struct FailedMergeOutput {
  pub remote_branch: String,
  pub pull_request_url: String,
}

#[derive(Debug)]
pub struct MergeAllTasksOutput {
  pub repo_path: String,
  pub tasks_in_master_branch: Vec<TaskInMasterBranch>,
  pub matched_task_branch_mappings: Vec<MatchedTaskBranchMapping>,
  pub successful_merge_task_operations: Vec<SuccesfulMergeOutput>,
  pub failed_merge_task_operations: Vec<FailedMergeOutput>,
}

#[derive(Debug)]
pub struct MergeFeatureBranchesOutput {
  pub merge_all_tasks_outputs: Vec<MergeAllTasksOutput>,
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

impl GlobalDeploymentClient {
  pub async fn deploy(&self, repo_key: &str, scheme_key: &str) -> ResultDynError<()> {
    let repo_deployment_client = self
      .repository_deployment_client_by_key
      .get(repo_key)
      .ok_or_else(|| {
        return failure::err_msg(format!("Invalid repo key {}", repo_key));
      })?;

    return repo_deployment_client
      .deploy(scheme_key, GithubMergeMethod::Merge)
      .await;
  }

  pub async fn merge_feature_branches(
    &self,
    task_ids: &Vec<&str>,
  ) -> ResultDynError<MergeFeatureBranchesOutput> {
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

    let mut merge_results: Vec<ResultDynError<MergeAllTasksOutput>> = vec![];

    for deployment_client in self.repository_deployment_client_by_key.values() {
      let merge_result = deployment_client.merge_all_tasks(task_ids).await;

      merge_results.push(merge_result);
    }

    // Make sure that all is well
    let merge_results: ResultDynError<Vec<MergeAllTasksOutput>> =
      merge_results.into_iter().collect();
    let merge_results = merge_results?;
    let not_found_user_task_mappings =
      TaskUtil::find_not_found_tasks(&merge_results, &task_by_id, &task_assignee_by_phid);

    return Ok(MergeFeatureBranchesOutput {
      merge_all_tasks_outputs: merge_results,
      not_found_user_task_mappings,
      task_by_id,
    });
  }
}

struct RepositoryDeploymentClient {
  pub config: RepositoryConfig,
  ghub: Arc<GithubClient>,
  logger: Arc<Logger>,
  preset_command: PresetCommand,
}

impl RepositoryDeploymentClient {
  fn new(
    config: RepositoryConfig,
    ghub: Arc<GithubClient>,
    logger: Logger,
  ) -> RepositoryDeploymentClient {
    return RepositoryDeploymentClient {
      config: config.clone(),
      ghub,
      logger: Arc::new(logger),
      preset_command: PresetCommand {
        working_dir: config.path.clone(),
      },
    };
  }
}

impl RepositoryDeploymentClient {
  pub async fn merge_remote_branch(
    &self,
    pull_request_title: &str,
    source_branch_name: &str,
    into_branch_name: &str,
    merge_method: github_pull_request::GithubMergeMethod,
  ) -> ResultDynError<SuccesfulMergeOutput> {
    let repo_path = &self.config.github_path;

    let mut pull_request: Option<Value> = self
      .ghub
      .pull_request
      .get_by_head(github_pull_request::GetPullRequestByHeadInput {
        repo_path,
        branch_name: source_branch_name,
        branch_owner: repo_path
          .split('/')
          .nth(0)
          .ok_or(format!("Could not read branch owner from {}", repo_path))
          .map_err(failure::err_msg)?,
      })
      .await?;

    if pull_request.is_none() {
      let input = github_pull_request::CreatePullRequestInput {
        title: pull_request_title,
        repo_path,
        branch_name: source_branch_name,
        into_branch: into_branch_name,
      };

      slog::info!(self.logger, "Creating PR {:?}", input);
      let res_body: ResultDynError<Value> = self.ghub.pull_request.create(input).await;
      slog::info!(self.logger, "Done creating PR");
      slog::debug!(self.logger, "Response body {:?}", res_body);

      pull_request = Some(res_body?);
    }

    let pull_request = pull_request.unwrap();
    let mergeable_str = format!("{}", pull_request["mergeable"]);
    let pull_number = &format!("{}", pull_request["number"]);
    let pull_request_url = format!("https://github.com/{}/pull/{}", repo_path, pull_number);

    // TODO: We need to poll periodically for this
    // https://developer.github.com/v3/git/#checking-mergeability-of-pull-requests
    if mergeable_str == "false" {
      return Err(
        ClientOperationError::MergeError {
          remote_branch: source_branch_name.into(),
          pull_request_url,
        }
        .into(),
      );
    }
    // }

    // Merge
    // -----------------------
    let input = github_pull_request::MergePullRequestInput {
      repo_path: &self.config.github_path,
      pull_number,
      merge_method,
    };

    slog::info!(self.logger, "Merging PR {:?}", input);
    let res_body: Value = self.ghub.pull_request.merge(input).await?;
    slog::info!(self.logger, "Done merging PR");
    slog::debug!(self.logger, "Response body {:?}", res_body);

    let merge_succeeded: bool = res_body["merged"].as_bool().ok_or(failure::err_msg(
      "Failed to parse merge pull request 'merged' to bool",
    ))?;

    if !merge_succeeded {
      return Err(
        ClientOperationError::MergeError {
          remote_branch: source_branch_name.into(),
          pull_request_url,
        }
        .into(),
      );
    }

    return Ok(SuccesfulMergeOutput {
      remote_branch: source_branch_name.into(),
      pull_request_url,
    });
  }

  /// As of now this only do merging.
  /// Will do deployment in the future~
  pub async fn deploy(
    &self,
    scheme_key: &str,
    merge_method: GithubMergeMethod,
  ) -> ResultDynError<()> {
    let scheme = self
      .config
      .deployment_scheme_by_key
      .get(scheme_key)
      .ok_or_else(|| {
        return failure::err_msg(format!("Invalid scheme key {}", scheme_key));
      })?;

    let _ = self
      .merge_remote_branch(
        &scheme.default_pull_request_title,
        &scheme.merge_from_branch,
        &scheme.merge_into_branch,
        merge_method,
      )
      .await;

    return Ok(());
  }

  pub async fn merge_all_tasks(&self, task_ids: &Vec<&str>) -> ResultDynError<MergeAllTasksOutput> {
    slog::info!(self.logger, "[Run] git checkout master");

    slog::info!(
      self.logger,
      "{}",
      self.preset_command.exec("git checkout master")?
    );

    slog::info!(self.logger, "[Run] git pull origin master");

    slog::info!(
      self.logger,
      "{}",
      self.preset_command.exec("git pull origin master")?
    );

    // This will sync deleted branch remotely, sometimes we've deleted remote branch
    // but it still appears locally under origin/<branchname> when running `git branch -r`.
    slog::info!(self.logger, "[Run] git remote prune origin");
    slog::info!(
      self.logger,
      "{}",
      self.preset_command.exec("git remote prune origin")?
    );

    slog::info!(self.logger, "[Run] git fetch --all");

    slog::info!(
      self.logger,
      "{}",
      self.preset_command.exec("git fetch --all")?
    );

    slog::info!(self.logger, "[Run] git branch -r");

    let remote_branches = self.preset_command.exec("git branch -r")?;

    let filtered_branch_mappings: Vec<MatchedTaskBranchMapping> =
      TaskUtil::create_matching_task_and_branch(task_ids, &remote_branches.split('\n').collect());
    let tasks_in_master_branch = self.tasks_in_master_branch(&task_ids).await?;

    let all: Vec<futures::future::BoxFuture<Result<SuccesfulMergeOutput, _>>> =
      filtered_branch_mappings
        .iter()
        .map(|MatchedTaskBranchMapping(_task_id, remote_branch)| {
          async move {
            return self.merge(&remote_branch).await;
          }
          .boxed()
        })
        .collect();

    let results: Vec<ResultDynError<SuccesfulMergeOutput>> = futures::future::join_all(all).await;
    let show_stopper_error: Option<&failure::Error> = results.iter().find_map(
      |result: &Result<SuccesfulMergeOutput, failure::Error>| -> Option<&failure::Error> {
        return result.as_ref().err().filter(|err| {
          let maybe_merge_error: Option<&ClientOperationError> = err.downcast_ref();

          return maybe_merge_error.is_none();
        });
      },
    );

    if show_stopper_error.is_some() {
      // let show_stopper_error: failure::Error =
      // failure::Error::from(show_stopper_error.unwrap().as_fail().into());

      return Err(failure::err_msg(format!("{}", show_stopper_error.unwrap())));
    }

    let (successes, failures): (
      Vec<ResultDynError<SuccesfulMergeOutput>>,
      Vec<ResultDynError<SuccesfulMergeOutput>>,
    ) = results.into_iter().partition(|result| result.is_ok());

    let failed_merge_task_operations: Vec<_> = failures
      .into_iter()
      .map(|possible_merge_error| {
        let err = possible_merge_error.err().unwrap();
        let ClientOperationError::MergeError {
          remote_branch,
          pull_request_url,
        } = err.downcast_ref().unwrap();

        return FailedMergeOutput {
          remote_branch: String::from(remote_branch),
          pull_request_url: String::from(pull_request_url),
        };
      })
      .collect();

    let successful_merge_task_operations = successes.into_iter().map(Result::unwrap).collect();

    return Ok(MergeAllTasksOutput {
      tasks_in_master_branch,
      matched_task_branch_mappings: filtered_branch_mappings,
      repo_path: self.config.path.clone(),
      successful_merge_task_operations,
      failed_merge_task_operations,
    });
  }

  fn get_pull_request_title(&self, remote_branch: &str) -> ResultDynError<String> {
    let commit_messages = self.preset_command.exec(&format!(
      "git log --oneline --pretty=format:%s master..{}",
      remote_branch
    ))?;

    return commit_messages
      .split('\n')
      .last()
      .ok_or(failure::format_err!(
        "Failed to get pull request title from remote branch {}",
        remote_branch
      ))
      .map(|pr_title| pr_title.to_owned());
  }

  async fn merge(&self, remote_branch: &str) -> ResultDynError<SuccesfulMergeOutput> {
    // Create PR
    // -----------------------
    let branch_name = remote_branch.split('/').last().ok_or(failure::format_err!(
      "Could not get branch name from {}",
      remote_branch
    ))?;

    let merge_output = self
      .merge_remote_branch(
        &self.get_pull_request_title(remote_branch)?,
        branch_name,
        "master",
        github_pull_request::GithubMergeMethod::Squash,
      )
      .await?;

    // Cleanup branch after squash merge to prevent
    // multiple merges
    self
      .ghub
      .branch
      .delete(DeleteBranchInput {
        repo_path: &self.config.github_path,
        branch_name,
      })
      .await?;

    return Ok(merge_output);
  }

  async fn tasks_in_master_branch(
    &self,
    task_ids: &Vec<&str>,
  ) -> ResultDynError<Vec<TaskInMasterBranch>> {
    let git_log_handle = self.preset_command.spawn_command_from_str(
      "git log --oneline",
      None,
      Some(Stdio::piped()),
    )?;

    let grep_regex_input = task_ids.iter().fold("".to_owned(), |acc, task_id| {
      if acc.is_empty() {
        return String::from(*task_id);
      }

      return format!("{}\\|{}", acc, task_id);
    });

    let grep_output = self
      .preset_command
      .spawn_command_from_str(
        &format!("grep {}", grep_regex_input),
        Some(git_log_handle.stdout.unwrap().into()),
        None,
      )?
      .wait_with_output()?;

    let grep_output = command::handle_command_output(grep_output)?;
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
    merge_results: &Vec<MergeAllTasksOutput>,
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

      let merge_results: Vec<MergeAllTasksOutput> = vec![MergeAllTasksOutput {
        repo_path: String::from("/foo"),
        tasks_in_master_branch: vec![],
        matched_task_branch_mappings: vec![MatchedTaskBranchMapping(
          "3333".into(),
          "origin/bar_T3333_foo".into(),
        )],
        successful_merge_task_operations: vec![SuccesfulMergeOutput {
          remote_branch: "origin/bar_T3333_foo".into(),
          pull_request_url: "https://example.com".into(),
        }],
        failed_merge_task_operations: vec![],
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

use std::collections::HashMap;
use std::convert::TryInto;
use std::process::Stdio;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Error;
use futures::FutureExt;
use futures::StreamExt;
use ghub::v3::branch::DeleteBranchInput;
use ghub::v3::client::GithubClient;
use ghub::v3::pull_request as github_pull_request;
use ghub::v3::pull_request::GithubMergeMethod;
use phab_lib::client::config::CertIdentityConfig;
use phab_lib::client::config::PhabricatorClientConfig;
use phab_lib::client::phabricator::PhabricatorClient;
use phab_lib::dto::Task;
use phab_lib::dto::User;
use serde::Serialize;
use serde_json::Value;
use slog::Logger;

use crate::config::Config;
use crate::config::RepositoryConfig;

use lezeh_common::command;
use lezeh_common::command::PresetCommand;
use lezeh_common::types::ResultAnyError;

pub struct GlobalDeploymentClient {
  pub config: Config,
  phabricator: Arc<PhabricatorClient>,
  repository_deployment_client_by_key: HashMap<String, RepositoryDeploymentClient>,

  #[allow(dead_code)]
  ghub: Arc<GithubClient>,

  #[allow(dead_code)]
  logger: &'static Logger,
}

impl GlobalDeploymentClient {
  pub fn new(config: Config, logger: &'static Logger) -> ResultAnyError<GlobalDeploymentClient> {
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

#[derive(Debug, Clone, thiserror::Error)]
pub enum GitError {
  #[error("Merge failed err: {message}. Please see {pull_request_url}")]
  MergeError {
    message: String,
    remote_branch: String,
    pull_request_url: String,
  },
  #[error("Remote branch is behind master(no changes to master), remote branch {remote_branch}")]
  RemoteBranchIsBehindMasterError {
    remote_branch: String,
    debug_url: String,
  },
}

#[derive(Debug, Serialize)]
pub struct SuccesfulMergeOutput {
  pub remote_branch: String,
  pub pull_request_url: String,
}

#[derive(Debug, Serialize)]
pub struct SuccesfulMergeTaskOutput {
  pub repo_config: RepositoryConfig,
  pub task_id: String,
  pub remote_branch: String,
  pub pull_request_url: String,
}

#[derive(Debug, Serialize)]
pub struct FailedMergeTaskOutput {
  pub repo_config: RepositoryConfig,
  pub task_id: String,
  pub remote_branch: String,
  pub debug_url: String,
  pub message: String,
}

#[derive(Debug, Serialize)]
pub struct MergeAllTasksOutput {
  pub repo_path: String,
  pub tasks_in_master_branch_by_task_id: HashMap<String, Vec<TaskInMasterBranch>>,
  pub matched_task_branch_mappings: Vec<MatchedTaskBranchMapping>,
  pub successful_merge_task_output_by_task_id: HashMap<String, SuccesfulMergeTaskOutput>,
  pub failed_merge_task_output_by_task_id: HashMap<String, FailedMergeTaskOutput>,
}

#[derive(Debug, Serialize)]
pub struct MergeFeatureBranchesOutput {
  pub merge_all_tasks_outputs: Vec<MergeAllTasksOutput>,
  pub task_by_id: HashMap<String, Task>,
  pub found_task_by_id: HashMap<String, Task>,
  pub not_found_user_task_mappings: Vec<UserTaskMapping>,
}

#[derive(Debug, Serialize)]
pub struct UserTaskMapping(pub User, pub Task);

#[derive(Debug, Serialize)]
pub struct MatchedTaskBranchMapping(pub String, pub String);

#[derive(Debug, Serialize)]
pub struct TaskInMasterBranch {
  pub repo_config: RepositoryConfig,
  pub commit_message: String,
  pub task_id: String,
}

impl GlobalDeploymentClient {
  pub async fn deploy(&self, repo_key: &str, scheme_key: &str) -> ResultAnyError<()> {
    let repo_deployment_client = self
      .repository_deployment_client_by_key
      .get(repo_key)
      .ok_or_else(|| {
        return anyhow!("Invalid repo key {}", repo_key);
      })?;

    return repo_deployment_client
      .deploy(scheme_key, GithubMergeMethod::Merge)
      .await;
  }

  pub async fn merge_feature_branches(
    &self,
    task_ids: &Vec<&str>,
    concurrency_limit: usize,
  ) -> ResultAnyError<MergeFeatureBranchesOutput> {
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

    // Create async tasks that will be run in parallel.
    let tasks = self
      .repository_deployment_client_by_key
      .values()
      .map(|deployment_client| {
        return deployment_client.merge_all_tasks(&task_by_id);
      });

    let merge_results: Vec<ResultAnyError<MergeAllTasksOutput>> = futures::stream::iter(tasks)
      .buffered(concurrency_limit)
      .collect()
      .await;

    // Make sure that all is well
    let merge_results: ResultAnyError<Vec<MergeAllTasksOutput>> =
      merge_results.into_iter().collect();
    let merge_results = merge_results?;
    let not_found_user_task_mappings =
      TaskUtil::find_not_found_tasks(&merge_results, &task_by_id, &task_assignee_by_phid);

    let found_task_by_id: HashMap<String, Task> = task_by_id
      .iter()
      .filter(|(task_id, _)| {
        return not_found_user_task_mappings
          .iter()
          .find(|UserTaskMapping(_user, not_found_task)| {
            return not_found_task.id == **task_id;
          })
          .is_none();
      })
      .map(|(key, task_reference): (&String, &Task)| {
        return (key.clone(), task_reference.clone());
      })
      .collect();

    return Ok(MergeFeatureBranchesOutput {
      merge_all_tasks_outputs: merge_results,
      not_found_user_task_mappings,
      found_task_by_id,
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

struct GetPullRequestInput<'a> {
  pub repo_path: &'a str,
  pub branch_name: &'a str,
}

impl RepositoryDeploymentClient {
  async fn get_pull_request<'a>(
    &self,
    input: GetPullRequestInput<'a>,
  ) -> ResultAnyError<Option<Value>> {
    let GetPullRequestInput {
      repo_path,
      branch_name,
    } = input;

    return self
      .ghub
      .pull_request
      .get_by_head(github_pull_request::GetPullRequestByHeadInput {
        repo_path,
        branch_name,
        branch_owner: repo_path
          .split('/')
          .nth(0)
          .ok_or(anyhow!("Could not read branch owner from {}", repo_path))?,
      })
      .await;
  }

  pub async fn merge_remote_branch(
    &self,
    pull_request_title: &str,
    source_branch_name: &str,
    into_branch_name: &str,
    merge_method: github_pull_request::GithubMergeMethod,
  ) -> ResultAnyError<SuccesfulMergeOutput> {
    let repo_path = &self.config.github_path;

    let mut pull_request: Option<Value> = self
      .get_pull_request(GetPullRequestInput {
        repo_path,
        branch_name: source_branch_name,
      })
      .await?;

    // Create pull request if there's none of it yet.
    if pull_request.is_none() {
      let input = github_pull_request::CreatePullRequestInput {
        title: pull_request_title,
        repo_path,
        branch_name: source_branch_name,
        into_branch: into_branch_name,
      };

      slog::info!(self.logger, "Creating PR {:?}", input);

      // Add this point creating pull request might fail due to many things.
      // One of the case that we should handle is when
      // the remote branch is behind master branch, in other words, the remote
      // branch does not have any commits to be merged. This can happen
      // because of 2 things:
      // A) It's already merged but the remote branch is not cleaned up yet
      // B) People just create remote branch but haven't pushed into it yet.
      //
      // The easiest way is to just return a specialized error
      // so the caller can handle this case.
      let res_body: Value = self.ghub.pull_request.create(input).await.map_err(|err| {
        if err
          .to_string()
          .to_lowercase()
          .starts_with("no commits between master")
        {
          let remote_branch: String = source_branch_name.into();

          return GitError::RemoteBranchIsBehindMasterError {
            remote_branch: remote_branch.clone(),
            debug_url: format!("https://github.com/{}/tree/{}", repo_path, remote_branch),
          }
          .into();
        }

        return err;
      })?;

      slog::info!(self.logger, "Done creating PR {:?}", res_body);
      slog::debug!(self.logger, "Response body {:?}", res_body);

      // Wait for 2 seconds to give github sometime to calculate mergeability
      tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;

      // We're refetching the PR to trigger a mergeability check on github
      // https://developer.github.com/v3/git/#checking-mergeability-of-pull-requests
      pull_request = self
        .get_pull_request(GetPullRequestInput {
          repo_path,
          branch_name: source_branch_name,
        })
        .await?;
    }

    let pull_request = pull_request.unwrap();

    let mergeable: Option<bool> = pull_request["mergeable"].as_bool();
    let pull_number = &format!("{}", pull_request["number"]);
    let pull_request_url = format!("https://github.com/{}/pull/{}", repo_path, pull_number);

    if mergeable.is_some() && !mergeable.unwrap() {
      return Err(
        GitError::MergeError {
          message: format!("mergeable field is falsy ({})", mergeable.unwrap()),
          remote_branch: source_branch_name.into(),
          pull_request_url,
        }
        .into(),
      );
    }

    if mergeable.is_none() {
      slog::warn!(
        self.logger,
        "Could not reat mergeable will try to proceed, it should be safe because it will throw error if it's not mergeable from github side"
      )
    }

    // Merge
    // -----------------------
    let input = github_pull_request::MergePullRequestInput {
      repo_path: &self.config.github_path,
      pull_number,
      merge_method,
    };

    slog::info!(self.logger, "Merging PR {:?}", input);

    let res_body: Value = self.ghub.pull_request.merge(input).await.map_err(|err| {
      // This is to handle merge error when we can't read `mergeable` field,
      // we'll just rewrap the error so the merge sequence does not stop.
      return GitError::MergeError {
        message: err.to_string(),
        remote_branch: source_branch_name.into(),
        pull_request_url: pull_request_url.clone(),
      };
    })?;

    slog::info!(self.logger, "Done merging PR");
    slog::debug!(self.logger, "Response body {:?}", res_body);

    let merge_succeeded: bool = res_body["merged"].as_bool().ok_or(anyhow!(
      "Failed to parse merge pull request 'merged' to bool",
    ))?;

    if !merge_succeeded {
      return Err(
        GitError::MergeError {
          message: "Not sure why".into(),
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
  ) -> ResultAnyError<()> {
    let scheme = self
      .config
      .deployment_scheme_by_key
      .get(scheme_key)
      .ok_or_else(|| {
        return anyhow!("Invalid scheme key {}", scheme_key);
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

  pub async fn merge_all_tasks(
    &self,
    task_by_id: &HashMap<String, Task>,
  ) -> ResultAnyError<MergeAllTasksOutput> {
    // slog::info!(self.logger, "HAA");
    // tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    // slog::info!(self.logger, "HOOO");

    slog::info!(self.logger, "[Run] git checkout master");

    slog::info!(
      self.logger,
      "{}",
      self.preset_command.exec("git checkout master").await?
    );

    slog::info!(self.logger, "[Run] git pull origin master");

    slog::info!(
      self.logger,
      "{}",
      self.preset_command.exec("git pull origin master").await?
    );

    // This will sync deleted branch remotely, sometimes we've deleted remote branch
    // but it still appears locally under origin/<branchname> when running `git branch -r`.
    slog::info!(self.logger, "[Run] git remote prune origin");
    slog::info!(
      self.logger,
      "{}",
      self.preset_command.exec("git remote prune origin").await?
    );

    slog::info!(self.logger, "[Run] git fetch --all");

    slog::info!(
      self.logger,
      "{}",
      self.preset_command.exec("git fetch --all").await?
    );

    slog::info!(self.logger, "[Run] git branch -r");

    let remote_branches = self.preset_command.exec("git branch -r").await?;
    let task_ids: Vec<&str> = task_by_id.keys().map(String::as_ref).collect();

    let filtered_branch_mappings: Vec<MatchedTaskBranchMapping> =
      TaskUtil::create_matching_task_and_branch(&task_ids, &remote_branches.split('\n').collect());

    let tasks_in_master_branch_by_task_id =
      self.tasks_in_master_branch_by_task_id(&task_ids).await?;

    let all: Vec<futures::future::BoxFuture<(String, ResultAnyError<SuccesfulMergeOutput>)>> =
      filtered_branch_mappings
        .iter()
        .map(|MatchedTaskBranchMapping(task_id, remote_branch)| {
          async move {
            return (
              task_id.clone(),
              self
                .merge(
                  &format!(
                    "[{}] {}",
                    remote_branch
                      .split('/')
                      .nth(1)
                      .or(Some(task_id.as_ref()))
                      .unwrap(),
                    task_by_id.get(task_id).unwrap().name
                  ),
                  &remote_branch,
                )
                .await,
            );
          }
          .boxed()
        })
        .collect();

    let mut results: Vec<(String, ResultAnyError<SuccesfulMergeOutput>)> = vec![];

    // Merge in serially instead of concurrently to reduce possibility
    // of race conditions.
    for fut in all.into_iter() {
      results.push(fut.await);
    }

    let show_stopper_error: Option<&Error> = results.iter().find_map(
      |(_task_id, result): &(String, ResultAnyError<SuccesfulMergeOutput>)| -> Option<&Error> {
        return result.as_ref().err().filter(|err| {
          let maybe_merge_error: Option<&GitError> = err.downcast_ref();

          return maybe_merge_error.is_none();
        });
      },
    );

    if show_stopper_error.is_some() {
      return Err(anyhow!(format!("{}", show_stopper_error.unwrap())));
    }

    let (successes, failures): (
      Vec<(String, ResultAnyError<SuccesfulMergeOutput>)>,
      Vec<(String, ResultAnyError<SuccesfulMergeOutput>)>,
    ) = results
      .into_iter()
      .partition(|(_task_id, result)| result.is_ok());

    let failed_merge_task_output_by_task_id: HashMap<_, _> = failures
      .into_iter()
      .map(
        |(task_id, possible_merge_error): (String, ResultAnyError<SuccesfulMergeOutput>)| -> (String, FailedMergeTaskOutput) {
          let err = possible_merge_error.err().unwrap();
          let client_operation_error: &GitError = err.downcast_ref().unwrap();

          let (remote_branch, debug_url) = match client_operation_error {
            GitError::MergeError{
              message: _,
              remote_branch,
              pull_request_url
            } => (remote_branch, pull_request_url),
            GitError::RemoteBranchIsBehindMasterError{
              remote_branch,
              debug_url
            } => (remote_branch, debug_url),
          };

          return (
            task_id.clone(),
            FailedMergeTaskOutput {
              repo_config: self.config.clone(),
              task_id: task_id.clone(),
              remote_branch: String::from(remote_branch),
              debug_url: String::from(debug_url),
              message: client_operation_error.to_string()
            },
          );
        },
      )
      .collect();

    let successful_merge_task_output_by_task_id: HashMap<_, _> = successes
      .into_iter()
      .map(|(task_id, successful_merge_branch_output)| {
        let successful_merge_branch_output = successful_merge_branch_output.unwrap();

        return (
          task_id.clone(),
          SuccesfulMergeTaskOutput {
            repo_config: self.config.clone(),
            task_id: task_id.clone(),
            remote_branch: successful_merge_branch_output.remote_branch,
            pull_request_url: successful_merge_branch_output.pull_request_url,
          },
        );
      })
      .collect();

    return Ok(MergeAllTasksOutput {
      tasks_in_master_branch_by_task_id,
      matched_task_branch_mappings: filtered_branch_mappings,
      repo_path: self.config.github_path.clone(),
      successful_merge_task_output_by_task_id,
      failed_merge_task_output_by_task_id,
    });
  }

  async fn merge(
    &self,
    pull_request_title: &str,
    remote_branch: &str,
  ) -> ResultAnyError<SuccesfulMergeOutput> {
    // Create PR
    // -----------------------
    let branch_name = remote_branch
      .split('/')
      .last()
      .ok_or(anyhow!("Could not get branch name from {}", remote_branch))?;

    let merge_output = self
      .merge_remote_branch(
        pull_request_title,
        branch_name,
        "master",
        github_pull_request::GithubMergeMethod::Merge,
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

  async fn tasks_in_master_branch_by_task_id(
    &self,
    task_ids: &Vec<&str>,
  ) -> ResultAnyError<HashMap<String, Vec<TaskInMasterBranch>>> {
    let git_log_handle = self
      .preset_command
      .spawn_command_from_str(
        "git log --oneline --no-decorate", // In format {abbreviatedHash} {message}
        None,
        Some(Stdio::piped()),
      )
      .await?;

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
        Some(git_log_handle.stdout.unwrap().try_into()?),
        None,
      )
      .await?
      .wait_with_output()
      .await?;

    let grep_output = command::handle_command_output(grep_output)?;
    let commit_messages: Vec<&str> = grep_output
      .lines()
      .filter(|line| {
        return !line.contains("Merge pull request");
      })
      .collect();

    // Iterate all task ids and find which one
    // is in the commit messages, double loop here
    let task_id_commit_message_pairs: Vec<(String, String)> = task_ids
      .iter()
      .flat_map(|task_id| -> Vec<(String, String)> {
        // First loop
        return commit_messages
          .iter()
          .filter_map(|commit_message: &&str| -> Option<(String, String)> {
            // 2nd loop
            if !commit_message.contains(task_id) {
              return None;
            }

            return Some((String::from(*task_id), String::from(*commit_message)));
          })
          .collect();
      })
      .collect();

    let mut tasks_in_master_branch_by_id: HashMap<String, Vec<TaskInMasterBranch>> =
      Default::default();

    for (task_id, commit_message) in task_id_commit_message_pairs {
      tasks_in_master_branch_by_id
        .entry(task_id.clone())
        .or_insert(Default::default())
        .push(TaskInMasterBranch {
          repo_config: self.config.clone(),
          task_id,
          commit_message,
        });
    }

    return Ok(tasks_in_master_branch_by_id);
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

      for (task_id, _) in merge_result.tasks_in_master_branch_by_task_id.iter() {
        let current_counter = found_task_count_by_id
          .get_mut(PhabricatorClient::clean_id(task_id))
          .unwrap();

        *current_counter += 1;
      }
    });

    let not_found_user_task_mappings: Vec<UserTaskMapping> = found_task_count_by_id
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

      let mut successful_merge_task_output_by_task_id: HashMap<String, SuccesfulMergeTaskOutput> =
        HashMap::new();
      successful_merge_task_output_by_task_id.insert(
        "3333".to_owned(),
        SuccesfulMergeTaskOutput {
          repo_config: RepositoryConfig {
            key: "".to_owned(),
            path: "".to_owned(),
            github_path: "".to_owned(),
            deployment_scheme_by_key: HashMap::new(),
          },
          task_id: "3333".to_owned(),
          remote_branch: "origin/bar_T3333_foo".into(),
          pull_request_url: "https://example.com".into(),
        },
      );

      let merge_results: Vec<MergeAllTasksOutput> = vec![MergeAllTasksOutput {
        repo_path: String::from("/foo"),
        tasks_in_master_branch_by_task_id: Default::default(),
        matched_task_branch_mappings: vec![MatchedTaskBranchMapping(
          "3333".into(),
          "origin/bar_T3333_foo".into(),
        )],
        successful_merge_task_output_by_task_id,
        failed_merge_task_output_by_task_id: HashMap::new(),
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

use std::collections::HashMap;
use std::collections::VecDeque;
use std::process::Command;
use std::process::Stdio;

use phab_lib::client::phabricator::CertIdentityConfig;
use phab_lib::client::phabricator::PhabricatorClient;
use phab_lib::dto::Task as PhabricatorTask;
use phab_lib::dto::User as PhabricatorUser;

use crate::config::Config;
use crate::types::ResultDynError;

struct MatchedTaskMapping(String, String);

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

impl DeploymentClient {
  pub async fn merge_all(&self, task_ids: Vec<&str>) -> ResultDynError<()> {
    println!("[Run] git pull origin master");
    exec("git pull origin master", "Cannot git pull origin master")?;

    println!("[Run] git fetch --all");
    exec("git fetch --all", "Cannot git fetch remote")?;

    println!("[Run] git branch -r");
    let remote_branches = exec("git branch -r", "Cannot get all remote branches")?;

    let filtered_branch_mappings: Vec<MatchedTaskMapping> = remote_branches
      .split('\n')
      .flat_map(|remote_branch| {
        let remote_branch = remote_branch.trim().to_owned();

        return task_ids
          .iter()
          .map(|task_id| {
            return MatchedTaskMapping(String::from(task_id.to_owned()), remote_branch.clone());
          })
          .collect::<Vec<MatchedTaskMapping>>();
      })
      .filter(|MatchedTaskMapping(task_id, remote_branch)| {
        return remote_branch.contains(&task_id[..]);
      })
      .collect();

    println!("Branches to be merged");

    let tasks: Vec<PhabricatorTask> = self.phabricator.get_tasks_by_ids(task_ids).await?;

    let task_by_id: HashMap<&str, &PhabricatorTask> =
      tasks.iter().map(|task| (task.id.as_ref(), task)).collect();

    let task_assignee_ids: Vec<&str> = tasks
      .iter()
      // For simplicity's sake, we can be sure that every task should
      // have been assigned to an engineer.
      .map(|task| task.assigned_phid.as_ref().unwrap().as_ref())
      .collect();

    let task_assignees: Vec<PhabricatorUser> = self
      .phabricator
      .get_users_by_phids(task_assignee_ids.iter().map(AsRef::as_ref).collect())
      .await?;

    let task_assignee_by_phid: HashMap<&str, &PhabricatorUser> = task_assignees
      .iter()
      .map(|user| (user.phid.as_ref(), user))
      .collect();

    for MatchedTaskMapping(task_id, remote_branch) in filtered_branch_mappings.iter() {
      let task_id: &str = task_id.as_ref();

      // Phabricator task id does not have prefix `T` as in `T1234`
      let task = task_by_id.get(PhabricatorClient::clean_id(task_id));

      if task.is_none() {
        println!("Could not find task {} from phabricator", task_id);
        continue;
      }

      let task = task.unwrap();
      let assigned_phid: &str = task.assigned_phid.as_ref().unwrap();

      // We can be sure task_assignee 100% exist because
      // we construct task_assignees based on tasks.
      let task_assignee = task_assignee_by_phid.get(assigned_phid).unwrap();

      println!(
        "{} - {}: {}",
        task_id, task_assignee.username, remote_branch
      );
    }

    println!("------------------------------------------");
    println!("------------------------------------------");

    for MatchedTaskMapping(_task_id, remote_branch) in filtered_branch_mappings.iter() {
      self.merge(&remote_branch)?;
    }

    return Ok(());
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
}

fn command_from_str(command_str: &str) -> ResultDynError<&'static mut Command> {
  let mut command_parts = command_str.split(' ').collect::<VecDeque<&str>>();

  let command = command_parts
    .pop_front()
    .ok_or(format!("Invalid command: {}", command_str))
    .map_err(failure::err_msg)?;

  let command = Command::new(command).args(command_parts);

  return Ok(command);
}

fn exec(command_str: &str, assertion_txt: &str) -> ResultDynError<String> {
  let command = command_from_str(command_str)?;

  let command_result = command.output().expect(assertion_txt);

  if !command_result.stderr.is_empty() {
    return std::str::from_utf8(&command_result.stderr)
      .map(String::from)
      .map_err(failure::err_msg);
  }

  return std::str::from_utf8(&command_result.stdout)
    .map(String::from)
    .map_err(failure::err_msg);
}

use std::collections::VecDeque;
use std::process::Command;

use phab_lib::client::phabricator::CertIdentityConfig;
use phab_lib::client::phabricator::PhabricatorClient;
use phab_lib::dto::User as PhabricatorUser;

use crate::config::Config;
use crate::types::ResultDynError;

struct MatchedTaskMapping(String, String);

pub struct DeploymentClient {
  pub config: Config,
}

impl DeploymentClient {
  pub fn new(config: Config) -> DeploymentClient {
    return DeploymentClient { config };
  }
}

impl DeploymentClient {
  pub fn merge_all(&self, task_ids: Vec<&str>) -> ResultDynError<()> {
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

    for MatchedTaskMapping(task_id, remote_branch) in filtered_branch_mappings.iter() {
      // TODO: Fetch task owner here using phab lib
      println!("{}: {}", task_id, remote_branch);
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

fn exec(command: &str, assertion_txt: &str) -> ResultDynError<String> {
  let mut command_parts = command.split(' ').collect::<VecDeque<&str>>();

  let cmd = command_parts
    .pop_front()
    .ok_or(format!("Invalid command: {}", command))
    .map_err(failure::err_msg)?;

  let command_result = Command::new(cmd)
    .args(command_parts)
    .output()
    .expect(assertion_txt);

  if !command_result.stderr.is_empty() {
    return std::str::from_utf8(&command_result.stderr)
      .map(String::from)
      .map_err(failure::err_msg);
  }

  return std::str::from_utf8(&command_result.stdout)
    .map(String::from)
    .map_err(failure::err_msg);
}

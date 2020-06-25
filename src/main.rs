use std::collections::HashSet;
use std::collections::VecDeque;
use std::process::Command;

use clap::App as Cli;
use clap::Arg;
use clap::ArgMatches;
use clap::SubCommand;

pub mod built_info {
  include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

type ResultDynError<T> = Result<T, Box<dyn std::error::Error>>;

fn main() -> ResultDynError<()> {
  let cli = Cli::new("Lezeh")
    .version(built_info::PKG_VERSION)
    .author(built_info::PKG_AUTHORS)
    .setting(clap::AppSettings::ArgRequiredElseHelp)
    .about(built_info::PKG_DESCRIPTION)
    .subcommand(deployment_cmd())
    .get_matches();

  if let Some(deployment_cli) = cli.subcommand_matches("deployment") {
    handle_deployment_cli(deployment_cli)?;
  }

  return Ok(());
}

fn deployment_cmd<'a, 'b>() -> Cli<'a, 'b> {
  let task_id_args = Arg::with_name("task_ids")
    .multiple(true)
    .required(true)
    .help("task ids");

  return SubCommand::with_name("deployment")
    .setting(clap::AppSettings::ArgRequiredElseHelp)
    .about("deployment cli")
    .subcommand(
      SubCommand::with_name("merge-all")
        .about("Rebase and merge all task ids")
        .arg(task_id_args),
    );
}

fn handle_deployment_cli(cli: &ArgMatches<'_>) -> ResultDynError<()> {
  if let Some(deployment_cli) = cli.subcommand_matches("merge-all") {
    let task_ids = deployment_cli
      .values_of("task_ids")
      .unwrap()
      .map(Into::into)
      .collect();

    merge_all(task_ids);
  }

  return Ok(());
}

fn merge_all(task_ids: Vec<&str>) {
  println!("[Run] git pull origin master");
  exec("git pull origin master", "Cannot git pull origin master");

  println!("[Run] git fetch --all");
  exec("git fetch --all", "Cannot git fetch remote");

  println!("[Run] git branch -r");
  let remote_branches = exec("git branch -r", "Cannot get all remote branches");

  let filtered_branches: HashSet<String> = remote_branches
    .split('\n')
    .into_iter()
    .map(|s| s.trim().to_owned())
    .filter(|remote_branch| {
      return !task_ids
        .iter()
        .filter(|task_id| {
          return remote_branch.contains(&task_id[..]);
        })
        .map(|x| String::from(*x))
        .collect::<Vec<String>>()
        .is_empty();
    })
    .collect();

  let filtered_branch_string = filtered_branches
    .iter()
    .collect::<Vec<&String>>()
    .into_iter()
    .fold("".to_owned(), |acc, branch| {
      return format!("{}\n{}", acc, branch);
    });

  println!("Branches to be merged: {}", filtered_branch_string);
  println!("------------------------------------------");

  for remote_branch in filtered_branches.iter() {
    merge(&remote_branch)
  }
}

fn merge(remote_branch: &str) {
  let splitted = remote_branch
    .split("/")
    .map(String::from)
    .collect::<Vec<String>>();

  let local_branch = splitted.get(1).unwrap();

  println!("[{}] Merging...", remote_branch);

  let namespace = format!("[{}]", local_branch);

  println!("{} git checkout {}", namespace, local_branch);
  exec(
    &format!("git checkout {}", local_branch),
    &format!("{} Cannot checkout", namespace),
  );

  println!("{} git rebase master", namespace);
  exec(
    "git rebase master",
    &format!("{} Cannot rebase master", namespace),
  );

  println!("{} git checkout master", namespace);
  exec(
    "git checkout master",
    &format!("{} Cannot checkout master", namespace),
  );

  println!("{} git merge {} --ff-only", namespace, local_branch);
  exec(
    &format!("git merge {} --ff-only", local_branch),
    &format!("{} Cannot checkout master", namespace),
  );
}

fn exec(command: &str, assertion_txt: &str) -> String {
  let mut command_parts = command.split(' ').collect::<VecDeque<&str>>();

  let cmd = command_parts
    .pop_front()
    .expect(&format!("Invalid command: {}", command));

  let stdout = Command::new(cmd)
    .args(command_parts)
    .output()
    .expect(assertion_txt)
    .stdout;

  return std::str::from_utf8(&stdout)
    .expect("Could not extract stdout")
    .to_owned();
}

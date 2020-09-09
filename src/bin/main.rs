use clap::App as Cli;
use clap::Arg;
use clap::ArgMatches;
use clap::SubCommand;

use lib::client::GlobalDeploymentClient;
use lib::config::Config;
use lib::types::ResultDynError;

use slog::*;

pub mod built_info {
  include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

#[tokio::main]
async fn main() -> ResultDynError<()> {
  env_logger::init();

  let log_decorator = slog_term::TermDecorator::new().build();
  let log_drain = slog_term::CompactFormat::new(log_decorator).build().fuse();
  let rust_log_val = std::env::var("RUST_LOG").unwrap_or("info".to_owned());
  let log_drain = slog_envlogger::LogBuilder::new(log_drain)
    .parse(&rust_log_val)
    .build();

  let log_drain = slog_async::Async::new(log_drain).build().fuse();

  let logger = slog::Logger::root(log_drain, o!());

  // Default config
  let home_dir = std::env::var("HOME").unwrap();
  let config = Config::new(format!("{}/.lezeh", home_dir))?;

  let cli = Cli::new("Lezeh")
    .version(built_info::PKG_VERSION)
    .author(built_info::PKG_AUTHORS)
    .setting(clap::AppSettings::ArgRequiredElseHelp)
    .about(built_info::PKG_DESCRIPTION)
    .subcommand(deployment_cmd())
    .get_matches();

  if let Some(deployment_cli) = cli.subcommand_matches("deployment") {
    handle_deployment_cli(deployment_cli, config, logger).await?;
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
      SubCommand::with_name("deploy")
      .about("Merge repo (given repo key) based on given deployment scheme config")
      .arg(Arg::with_name("repo_key")
        .required(true)
        .help("Repo key")
      )
      .arg(Arg::with_name("scheme_key")
        .required(true)
        .help("Deployment scheme key")
      )
    )
    .subcommand(
      SubCommand::with_name("merge-feature-branches")
        .about("Rebase and merge all feature branches for all repos in the config based on the given task ids")
        .arg(task_id_args),
    );
}

async fn handle_deployment_cli(
  cli: &ArgMatches<'_>,
  config: Config,
  logger: Logger,
) -> ResultDynError<()> {
  let deployment_client = GlobalDeploymentClient::new(config, logger)?;

  if let Some(merge_cli) = cli.subcommand_matches("deploy") {
    let repo_key: &str = merge_cli.value_of("repo_key").unwrap();
    let scheme_key: &str = merge_cli.value_of("scheme_key").unwrap();

    return deployment_client.deploy(repo_key, scheme_key).await;
  } else if let Some(merge_feature_branches_cli) = cli.subcommand_matches("merge-feature-branches")
  {
    let task_ids = merge_feature_branches_cli
      .values_of("task_ids")
      .unwrap()
      .map(Into::into)
      .collect();

    let output = deployment_client.merge_feature_branches(&task_ids).await?;

    println!("# Merge stats");
    println!("## Not found task ids");
    println!("================================");
    println!("================================");

    for lib::client::UserTaskMapping(user, task) in output.not_found_user_task_mappings.iter() {
      println!("T{}: {}", task.id, user.username);
    }

    println!("");
    println!("## Repo merge stats");
    println!("================================");
    println!("================================");
    for repo_merge_output in output.merge_all_tasks_outputs.iter() {
      println!("### Repo {}", repo_merge_output.repo_path);
      println!("--------------------------------");

      println!("#### Tasks in master branch");
      for task_in_master_branch in repo_merge_output.tasks_in_master_branch.iter() {
        println!(
          "{}: {}",
          task_in_master_branch.task_id, task_in_master_branch.commit_message
        );
      }

      println!("#### Matched tasks");
      for lib::client::MatchedTaskBranchMapping(task_id, remote_branch) in
        repo_merge_output.matched_task_branch_mappings.iter()
      {
        println!("{}: {}", task_id, remote_branch);
      }
    }
  }

  return Ok(());
}

use std::collections::HashMap;

use clap::App as Cli;
use clap::Arg;
use clap::ArgMatches;
use clap::SubCommand;

use lib::client::GlobalDeploymentClient;
use lib::client::MergeAllTasksOutput;
use lib::client::UserTaskMapping;
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
    let not_found_user_task_mapping_by_task_id: HashMap<String, &UserTaskMapping> = output
      .not_found_user_task_mappings
      .iter()
      .map(|user_task_mapping| {
        return (user_task_mapping.1.id.clone(), user_task_mapping);
      })
      .collect();

    let task_id_merge_infos: Vec<(String, String)> = output
      .merge_all_tasks_outputs
      .iter()
      .flat_map(|merge_all_tasks_output: &MergeAllTasksOutput| {
        let mut task_id_repo_master_branch_pairs: Vec<(String, String)> = merge_all_tasks_output
          .tasks_in_master_branch
          .iter()
          .map(|tasks_in_master_branch| {
            return (
              tasks_in_master_branch.task_id.clone(),
              format!(
                "🙌 [already in master] {}",
                merge_all_tasks_output.repo_path
              ),
            );
          })
          .collect();

        let mut task_id_successful_merge_task_pairs: Vec<(String, String)> = merge_all_tasks_output
          .successful_merge_task_operations
          .iter()
          .map(|successful_merge_output| {
            return (
              successful_merge_output.task_id.clone(),
              format!(
                "👌 [merged into master] {}",
                merge_all_tasks_output.repo_path
              ),
            );
          })
          .collect();

        let mut task_id_failed_merge_task_pairs: Vec<(String, String)> = merge_all_tasks_output
          .failed_merge_task_operations
          .iter()
          .map(|failed_merge_output| {
            return (
              failed_merge_output.task_id.clone(),
              format!(
                "👎 [merging failed] {} {}",
                merge_all_tasks_output.repo_path, failed_merge_output.pull_request_url
              ),
            );
          })
          .collect();

        let mut task_pairs: Vec<(String, String)> = vec![];
        task_pairs.append(&mut task_id_repo_master_branch_pairs);
        task_pairs.append(&mut task_id_successful_merge_task_pairs);
        task_pairs.append(&mut task_id_failed_merge_task_pairs);

        return task_pairs;
      })
      .collect();

    let mut merged_infos_by_task_id: HashMap<String, Vec<String>> = HashMap::new();

    for (task_id, merge_info) in task_id_merge_infos.into_iter() {
      merged_infos_by_task_id
        .entry(task_id)
        .or_insert(vec![])
        .push(merge_info);
    }

    for (task_id, merged_infos) in merged_infos_by_task_id.iter() {
      println!("📑 Task {}:", task_id);
      println!("=======================================");

      for merge_info in merged_infos.iter() {
        println!("{}", merge_info);
      }

      println!("\n");
    }

    println!("🛠  Not found tasks");
    println!("=======================================");
    let not_found_user_task_mapping_by_task_id: HashMap<String, &UserTaskMapping> =
      not_found_user_task_mapping_by_task_id
        .into_iter()
        .filter(|(task_id, _)| merged_infos_by_task_id.get(task_id).is_none())
        .collect();

    for (task_id, UserTaskMapping(user, _task)) in not_found_user_task_mapping_by_task_id.iter() {
      println!("🔮 Task {} - {}", task_id, user.username);
    }
  }

  return Ok(());
}

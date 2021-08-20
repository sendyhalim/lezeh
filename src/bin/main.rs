use std::collections::HashMap;

use clap::App as Cli;
use clap::Arg;
use clap::ArgMatches;
use clap::SubCommand;
use serde::Serialize;

use lib::clients::deployment_client::FailedMergeTaskOutput;
use lib::clients::deployment_client::GlobalDeploymentClient;
use lib::clients::deployment_client::SuccesfulMergeTaskOutput;
use lib::clients::deployment_client::TaskInMasterBranch;
use lib::clients::deployment_client::UserTaskMapping;
use lib::clients::url_client::LezehUrlClient;
use lib::config::Config;
use lib::renderers::handlebars::HandlebarsRenderer;
use lib::types::ResultDynError;

use slog::*;

pub mod built_info {
  include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

#[derive(Debug, Serialize)]
struct TaskMergeSummary<'a> {
  success_merge_results: Vec<&'a SuccesfulMergeTaskOutput>,
  failed_merge_results: Vec<&'a FailedMergeTaskOutput>,
  already_in_master_branch_related_commits: Vec<&'a TaskInMasterBranch>,
}

impl<'a> Default for TaskMergeSummary<'a> {
  fn default() -> Self {
    return TaskMergeSummary {
      success_merge_results: Default::default(),
      failed_merge_results: Default::default(),
      already_in_master_branch_related_commits: Default::default(),
    };
  }
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
    .subcommand(url_cmd())
    .get_matches();

  if let Some(deployment_cli) = cli.subcommand_matches("deployment") {
    handle_deployment_cli(deployment_cli, config, logger).await?;
  } else if let Some(url_cli) = cli.subcommand_matches("url") {
    handle_url_cli(url_cli, config).await?;
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

fn url_cmd<'a, 'b>() -> Cli<'a, 'b> {
  return SubCommand::with_name("url")
    .setting(clap::AppSettings::ArgRequiredElseHelp)
    .about("url cli")
    .subcommand(
      SubCommand::with_name("shorten")
        .about("Shorten the given url")
        .arg(Arg::with_name("long_url").required(true).help("Long Url")),
    );
}

async fn handle_deployment_cli(
  cli: &ArgMatches<'_>,
  config: Config,
  logger: Logger,
) -> ResultDynError<()> {
  let deployment_client = GlobalDeploymentClient::new(config.clone(), logger)?;

  if let Some(deploy_cli) = cli.subcommand_matches("deploy") {
    let repo_key: &str = deploy_cli.value_of("repo_key").unwrap();
    let scheme_key: &str = deploy_cli.value_of("scheme_key").unwrap();

    return deployment_client.deploy(repo_key, scheme_key).await;
  } else if let Some(merge_feature_branches_cli) = cli.subcommand_matches("merge-feature-branches")
  {
    let task_ids = merge_feature_branches_cli
      .values_of("task_ids")
      .unwrap()
      .map(Into::into)
      .collect();

    let merge_feature_branches_output = deployment_client.merge_feature_branches(&task_ids).await?;
    let not_found_user_task_mapping_by_task_id: HashMap<String, &UserTaskMapping> =
      merge_feature_branches_output
        .not_found_user_task_mappings
        .iter()
        .map(|user_task_mapping| {
          return (user_task_mapping.1.id.clone(), user_task_mapping);
        })
        .collect();

    let mut template_data: HashMap<&str, Box<dyn erased_serde::Serialize>> = HashMap::new();

    let mut merge_result_summary_by_task_id: HashMap<String, TaskMergeSummary> = Default::default();

    for (task_id, _) in merge_feature_branches_output.task_by_id.iter() {
      let task_summary = merge_result_summary_by_task_id
        .entry(task_id.clone())
        .or_default();

      for merge_all_task_output in merge_feature_branches_output.merge_all_tasks_outputs.iter() {
        let tasks_in_master_branch = merge_all_task_output
          .tasks_in_master_branch_by_task_id
          .get(task_id);

        if tasks_in_master_branch.is_some() {
          task_summary
            .already_in_master_branch_related_commits
            .extend(tasks_in_master_branch.unwrap());
        }

        let successful_merge_task = merge_all_task_output
          .successful_merge_task_output_by_task_id
          .get(task_id);

        if successful_merge_task.is_some() {
          task_summary
            .success_merge_results
            .push(successful_merge_task.unwrap());
        }

        let failed_merge_task = merge_all_task_output
          .failed_merge_task_output_by_task_id
          .get(task_id);

        if failed_merge_task.is_some() {
          task_summary
            .failed_merge_results
            .push(failed_merge_task.unwrap());
        }
      }
    }

    template_data.insert(
      "merge_result_summary_by_task_id",
      Box::from(merge_result_summary_by_task_id),
    );

    template_data.insert(
      "merge_feature_branches_output",
      Box::from(&merge_feature_branches_output),
    );

    template_data.insert(
      "not_found_user_task_mapping_by_task_id",
      Box::from(not_found_user_task_mapping_by_task_id),
    );

    let output: String = HandlebarsRenderer::new().render_from_template_path(
      &config
        .deployment
        .merge_feature_branches
        .unwrap()
        .output_template_path
        .unwrap(),
      template_data,
    )?;

    println!("{}", output);
  }

  return Ok(());
}

async fn handle_url_cli(cli: &ArgMatches<'_>, config: Config) -> ResultDynError<()> {
  let bitly_config = config
    .bitly
    .ok_or(failure::err_msg("Could not get bitly config"))?;

  let url_client = LezehUrlClient::new(bitly_config);

  if let Some(shorten_cli) = cli.subcommand_matches("shorten") {
    let long_url: &str = shorten_cli.value_of("long_url").unwrap();

    let short_url = url_client.shorten(long_url).await?;

    println!("{}", short_url);
  }

  return Ok(());
}

use std::collections::HashMap;

use clap::App as Cli;
use clap::Arg;
use clap::ArgMatches;
use clap::SubCommand;
use serde::Serialize;

use crate::client::FailedMergeTaskOutput;
use crate::client::GlobalDeploymentClient;
use crate::client::SuccesfulMergeTaskOutput;
use crate::client::TaskInMasterBranch;
use crate::client::UserTaskMapping;
use crate::config::Config;
use lezeh_common::handlebars::HandlebarsRenderer;
use lezeh_common::types::ResultAnyError;

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

pub struct DeploymentCli {}

impl DeploymentCli {
  pub fn cmd<'a, 'b>() -> Cli<'a, 'b> {
    let task_id_args = Arg::with_name("task_ids")
      .multiple(true)
      .required(true)
      .help("task ids");

    return Cli::new("deployment")
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

  pub async fn run(
    cli: &ArgMatches<'_>,
    config: Config,
    logger: &'static slog::Logger,
  ) -> ResultAnyError<()> {
    let deployment_client = GlobalDeploymentClient::new(config.clone(), logger)?;

    if let Some(deploy_cli) = cli.subcommand_matches("deploy") {
      let repo_key: &str = deploy_cli.value_of("repo_key").unwrap();
      let scheme_key: &str = deploy_cli.value_of("scheme_key").unwrap();

      return deployment_client.deploy(repo_key, scheme_key).await;
    } else if let Some(merge_feature_branches_cli) =
      cli.subcommand_matches("merge-feature-branches")
    {
      let task_ids = merge_feature_branches_cli
        .values_of("task_ids")
        .unwrap()
        .map(Into::into)
        .collect();

      let merge_feature_branches_output =
        deployment_client.merge_feature_branches(&task_ids).await?;
      let not_found_user_task_mapping_by_task_id: HashMap<String, &UserTaskMapping> =
        merge_feature_branches_output
          .not_found_user_task_mappings
          .iter()
          .map(|user_task_mapping| {
            return (user_task_mapping.1.id.clone(), user_task_mapping);
          })
          .collect();

      let mut template_data: HashMap<&str, Box<dyn erased_serde::Serialize>> = HashMap::new();

      let mut merge_result_summary_by_task_id: HashMap<String, TaskMergeSummary> =
        Default::default();

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
}

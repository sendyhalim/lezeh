use clap::App as Cli;
use clap::Arg;
use clap::ArgMatches;
use clap::SubCommand;

use lib::client::DeploymentClient;
use lib::config::Config;

pub mod built_info {
  include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

type ResultDynError<T> = Result<T, Box<dyn std::error::Error>>;

fn main() -> ResultDynError<()> {
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
    handle_deployment_cli(deployment_cli, config)?;
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

fn handle_deployment_cli(cli: &ArgMatches<'_>, config: Config) -> ResultDynError<()> {
  let deployment_client = DeploymentClient::new(config);

  if let Some(deployment_cli) = cli.subcommand_matches("merge-all") {
    let task_ids = deployment_cli
      .values_of("task_ids")
      .unwrap()
      .map(Into::into)
      .collect();

    deployment_client.merge_all(task_ids)?;
  }

  return Ok(());
}

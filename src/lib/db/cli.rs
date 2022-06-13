use clap::App as Cli;
use clap::Arg;
use clap::ArgMatches;
use clap::SubCommand;

use crate::common::config::Config;
use crate::common::types::ResultAnyError;

pub struct DbCli {}

impl DbCli {
  pub fn cmd<'a, 'b>() -> Cli<'a, 'b> {
    return Cli::new("db")
      .setting(clap::AppSettings::ArgRequiredElseHelp)
      .about("db cli")
      .subcommand(
        SubCommand::with_name("cherry-pick")
          .about("Cherry pick data from the given db source into the db target")
          .arg(
            Arg::with_name("table")
              .long("--table")
              .required(true)
              .help("Db table"),
          )
          .arg(
            Arg::with_name("ids")
              .long("--ids")
              .required(true)
              .help("Comma separated ids"),
          )
          .arg(
            Arg::with_name("source-db")
              .long("--source-db")
              .required(true)
              .help("Source db to fetch data from"),
          )
          .arg(
            Arg::with_name("target-db")
              .required(true)
              .long("--target-db")
              .help("Target db to insert db"),
          ),
      );
  }

  pub async fn run(cli: &ArgMatches<'_>, config: Config) -> ResultAnyError<()> {
    return Ok(());
  }
}

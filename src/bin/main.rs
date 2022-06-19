use std::collections::HashMap;
use std::collections::HashSet;

use clap::App as Cli;
use clap::Arg;
use clap::ArgMatches;

use lib::common::rose_tree::RoseTreeNode;
use lib::db::psql;
use lib::db::psql::dto::PsqlTableRows;

use lib::common::config::Config;
use lib::common::types::ResultAnyError;
use lib::db::cli::DbCli;
use lib::deployment::cli::DeploymentCli;
use lib::url::cli::UrlCli;

use slog::*;

pub mod built_info {
  include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

#[tokio::main]
async fn main() -> ResultAnyError<()> {
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
    .subcommand(DeploymentCli::cmd())
    .subcommand(UrlCli::cmd())
    .subcommand(DbCli::cmd())
    .get_matches();

  match cli.subcommand() {
    ("deployment", Some(cli)) => DeploymentCli::run(cli, config, logger).await?,
    ("url", Some(url_cli)) => UrlCli::run(url_cli, config).await?,
    ("db", Some(db_cli)) => DbCli::run(db_cli, config, logger).await?,
    _ => {}
  }

  return Ok(());
}

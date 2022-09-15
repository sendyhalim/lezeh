use clap::App as Cli;

use anyhow::anyhow;
use lezeh::config::Config;
use lezeh_bill::cli::BillCli;
use lezeh_common::logger;
use lezeh_common::types::ResultAnyError;
use lezeh_db::cli::DbCli;
use lezeh_deployment::cli::DeploymentCli;
use lezeh_url::cli::UrlCli;

pub mod built_info {
  include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

#[tokio::main]
async fn main() -> ResultAnyError<()> {
  let logger = logger::get();

  // Default config
  let home_dir = std::env::var("HOME").unwrap();
  let config = Config::new(format!("{}/.lezeh", home_dir))?;

  let cli = Cli::new("lezeh")
    .version(built_info::PKG_VERSION)
    .author(built_info::PKG_AUTHORS)
    .about(built_info::PKG_DESCRIPTION)
    .setting(clap::AppSettings::ArgRequiredElseHelp)
    .subcommand(DeploymentCli::cmd(Some("deployment")))
    .subcommand(UrlCli::cmd(Some("url")))
    .subcommand(DbCli::cmd(Some("db")))
    .subcommand(BillCli::cmd(Some("bill")))
    .get_matches();

  match cli.subcommand() {
    ("deployment", Some(cli)) => {
      DeploymentCli::run(
        cli,
        config
          .deployment
          .ok_or(anyhow!("deployment config is not set"))?,
        logger,
      )
      .await?
    }
    ("url", Some(url_cli)) => {
      UrlCli::run(url_cli, config.url.ok_or(anyhow!("url config is not set"))?).await?
    }
    ("db", Some(db_cli)) => {
      let db_cli = db_cli.clone();

      return tokio::task::spawn_blocking(move || {
        DbCli::run(
          &db_cli,
          config.db.ok_or(anyhow!("db config is not set"))?,
          logger,
        )
      })
      .await?;
    }
    ("bill", Some(bill_cli)) => {
      let bill_cli = bill_cli.clone();

      return tokio::task::spawn_blocking(move || BillCli::run(&bill_cli)).await?;
    }
    _ => {}
  }

  return Ok(());
}

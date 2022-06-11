use std::collections::HashMap;
use std::collections::HashSet;

use clap::App as Cli;
use clap::Arg;
use clap::ArgMatches;
use clap::SubCommand;

use lib::common::rose_tree::RoseTreeNode;
use lib::db::psql;
use lib::db::psql::dto::PsqlTableRows;

use lib::common::config::Config;
use lib::common::types::ResultAnyError;
use lib::deployment::cli::DeploymentCli;
use lib::url::cli::UrlCli;

use slog::*;

pub mod built_info {
  include!(concat!(env!("OUT_DIR"), "/built.rs"));
}

#[tokio::main]
async fn main() -> ResultAnyError<()> {
  env_logger::init();

  let handle = std::thread::spawn(|| {
    let psql = psql::connection::PsqlConnection::new(&lib::db::psql::connection::PsqlCreds {
      host: "localhost".to_owned(),
      database_name: "gepeel_app".to_owned(),
      username: "sendyhalim".to_owned(),
      password: Option::None,
    })
    .unwrap();

    let mut relation_fetcher = psql::relation_fetcher::RelationFetcher::new(psql);

    let psql_table_by_name = relation_fetcher.load_table_structure(Option::None).unwrap();

    let input = psql::relation_fetcher::FetchRowInput {
      schema: Some("public"),
      table_name: "store_staffs",
      column_name: "email",
      column_value: Box::new("bigpawofficial@gmail.com"),
    };

    let mut trees = relation_fetcher
      .fetch_rose_trees_to_be_inserted(&input, &psql_table_by_name)
      .unwrap();

    let tree: RoseTreeNode<PsqlTableRows> = trees.remove(0);
    println!("tree {:#?}", tree);

    let mut parents_by_level: HashMap<i32, HashSet<_>> =
      RoseTreeNode::parents_by_level(tree.clone());
    let children_by_level: HashMap<i32, HashSet<_>> =
      RoseTreeNode::children_by_level(tree, &mut parents_by_level);

    println!("{:#?}", parents_by_level);
    println!("{:#?}", children_by_level);
    let statements: Vec<String> =
      lib::db::psql::relation_insert::RelationInsert::into_insert_statements(parents_by_level);

    println!("{:#?}", statements);
  });

  handle.join().unwrap();

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
    .get_matches();

  match cli.subcommand() {
    ("deployment", Some(cli)) => DeploymentCli::run(cli, config, logger).await?,
    ("url", Some(url_cli)) => UrlCli::run(url_cli, config).await?,
    ("db", Some(db_cli)) => println!("Can't do nothign yet"),
    _ => {}
  }

  return Ok(());
}

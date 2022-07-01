use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::HashSet;
use std::rc::Rc;

use anyhow::anyhow;
use clap::App as Cli;
use clap::Arg;
use clap::ArgMatches;
use clap::SubCommand;
use slog::Logger;

use crate::common::config::Config;
use crate::common::config::DbConfig;
use crate::common::rose_tree::RoseTreeNode;
use crate::common::types::ResultAnyError;
use crate::db::psql;
use crate::db::psql::table_metadata::TableMetadata;

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
            Arg::with_name("schema")
              .long("--schema")
              .required(false)
              .takes_value(true)
              .help("Db schema"),
          )
          .arg(
            Arg::with_name("table")
              .long("--table")
              .required(true)
              .takes_value(true)
              .help("Db table"),
          )
          .arg(
            Arg::with_name("column")
              .long("--column")
              .required(false)
              .takes_value(true)
              .help("The column that the values are tied to, default to id"),
          )
          .arg(
            Arg::with_name("values")
              .long("--values")
              .required(true)
              .takes_value(true)
              .help("Comma separated values of the column to be fetched"),
          )
          .arg(
            Arg::with_name("source_db")
              .long("--source-db")
              .required(true)
              .takes_value(true)
              .help("Source db to fetch data from"),
          )
          .arg(
            Arg::with_name("target_db")
              .required(true)
              .takes_value(true)
              .long("--target-db")
              .help("Target db to insert db"),
          ),
      );
  }

  pub fn run(cli: &ArgMatches<'_>, config: Config, logger: Logger) -> ResultAnyError<()> {
    match cli.subcommand() {
      ("cherry-pick", Some(cherry_pick_cli)) => {
        let values: Vec<String> = cherry_pick_cli
          .values_of("values")
          .or_else(|| Default::default())
          .unwrap()
          .into_iter()
          .map(|s| s.to_owned())
          .collect();

        return DbCli::cherry_pick(
          cherry_pick_cli.value_of("source_db").unwrap(),
          cherry_pick_cli.value_of("target_db").unwrap(),
          cherry_pick_cli.value_of("table").unwrap(),
          values,
          cherry_pick_cli.value_of("column"),
          cherry_pick_cli.value_of("schema"),
          config,
          logger,
        );
      }
      _ => Ok(()),
    }
  }

  /// TODO:
  /// * Still broken, when passed criteria value need to check column type value
  ///   and then convert the given value based on column type value. So I think
  ///   we need to kind of get column metadata first and then convert based on
  ///   column spec
  /// * Refactor multiple .map on clis
  /// * Can we not spawn a new thread to just run it?
  /// * Need to apply the inserts on target db
  pub fn cherry_pick<'a>(
    source_db: &str,
    target_db: &str,
    table: &str,
    values: Vec<String>,
    column: Option<&str>,
    schema: Option<&str>,
    config: Config,
    logger: Logger,
  ) -> ResultAnyError<()> {
    let schema = schema.or(Some("public")).unwrap();
    let column = column.or(Some("id")).unwrap();

    let db_by_name: HashMap<String, DbConfig> = config
      .db_by_name
      .ok_or_else(|| anyhow!("Db config is not set"))?;

    let source_db_config: DbConfig = db_by_name
      .get(source_db)
      .ok_or_else(|| anyhow!("Source db {} is not registered", source_db))?
      .clone();

    let target_db_config: DbConfig = db_by_name
      .get(target_db)
      .ok_or_else(|| anyhow!("Target db {} is not registered", target_db))?
      .clone();

    let psql = psql::connection::PsqlConnection::new(&psql::connection::PsqlCreds {
      host: source_db_config.host.clone(),
      database_name: source_db_config.database.clone(),
      username: source_db_config.username.clone(),
      password: source_db_config.password.clone(),
    })?;

    let mut table_metadata = TableMetadata::new(Rc::new(RefCell::new(psql)));
    let psql_table_by_name = table_metadata.load_table_structure(schema)?;

    let mut relation_fetcher =
      psql::relation_fetcher::RelationFetcher::new(Rc::new(RefCell::new(table_metadata)));

    let input = psql::relation_fetcher::FetchRowsAsRoseTreeInput {
      schema: &schema,
      table_name: &table,
      column_name: &column,
      column_value: values.get(0).unwrap(), // As of now only supports 1 value
    };

    let trees = relation_fetcher.fetch_rose_trees_to_be_inserted(&input, &psql_table_by_name);

    if trees.is_err() {
      println!("{}", trees.unwrap_err());
      return Ok(());
    }

    let mut trees = trees?;

    // As of now only support 1 row
    let tree: RoseTreeNode<psql::dto::PsqlTableRows> = trees.remove(0);

    let mut nodes_by_level: HashMap<i32, HashSet<_>> = Default::default();

    // Prefill current rows
    let mut current_level_rows: HashSet<psql::dto::PsqlTableRows> = Default::default();
    current_level_rows.insert(tree.value.clone());
    nodes_by_level.insert(0, current_level_rows);

    // Populate parents
    nodes_by_level.extend(RoseTreeNode::parents_by_level(tree.clone()));

    // Populate children
    let children_by_level = RoseTreeNode::children_by_level(tree.clone(), &mut nodes_by_level);
    nodes_by_level.extend(children_by_level);

    let statements: Vec<String> =
      psql::relation_insert::RelationInsert::into_insert_statements(nodes_by_level)?;

    println!("{}", statements.join("\n"));

    return Ok(());
  }
}

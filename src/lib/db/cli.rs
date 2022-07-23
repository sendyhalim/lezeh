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
use crate::db::psql::connection::*;
use crate::db::psql::db_metadata::DbMetadata;
use crate::db::psql::dto::{PsqlTable, PsqlTableIdentity, PsqlTableRows};
use crate::db::psql::table_metadata::TableMetadataImpl;

pub struct DbCli {}

/// CLI definition
impl DbCli {
  pub fn cmd<'a, 'b>() -> Cli<'a, 'b> {
    return Cli::new("db")
      .setting(clap::AppSettings::ArgRequiredElseHelp)
      .about("db cli")
      .subcommand(
        SubCommand::with_name("cherry-pick")
          .about(indoc::indoc! {"
            Cherry pick row from the given db source and prints
            out insert statements for that specific row and all of its relations
            connected by foreign key.
          "})
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
          cherry_pick_cli.value_of("table").unwrap(),
          values,
          cherry_pick_cli.value_of("column").or(Some("id")).unwrap(),
          cherry_pick_cli
            .value_of("schema")
            .or(Some("public"))
            .unwrap(),
          config,
          logger,
        );
      }
      _ => Ok(()),
    }
  }
}

/// 1 method represents 1 CLI command
impl DbCli {
  fn cherry_pick<'a>(
    source_db: &str,
    table: &str,
    values: Vec<String>,
    column: &str,
    schema: &str,
    config: Config,
    logger: Logger,
  ) -> ResultAnyError<()> {
    let db_by_name: HashMap<String, DbConfig> = config
      .db_by_name
      .ok_or_else(|| anyhow!("Db config is not set"))?;

    let source_db_config: DbConfig = db_by_name
      .get(source_db)
      .ok_or_else(|| anyhow!("Source db {} is not registered", source_db))?
      .clone();

    let db_creds = PsqlCreds {
      host: source_db_config.host.clone(),
      database_name: source_db_config.database.clone(),
      username: source_db_config.username.clone(),
      password: source_db_config.password.clone(),
    };

    let psql = Rc::new(RefCell::new(PsqlConnection::new(&db_creds)?));
    let db_metadata = DbMetadata::new(psql.clone());
    let psql_table_by_id = db_metadata.load_table_structure(schema)?;

    let tree = DbCli::fetch_snowflake_relation(
      psql.clone(),
      &psql_table_by_id,
      table,
      values,
      column,
      schema,
    )?;

    let nodes_by_level: HashMap<i32, HashSet<_>> = RoseTreeNode::nodes_by_level(tree);

    let statements: Vec<String> =
      psql::relation_insert::RelationInsert::into_insert_statements(nodes_by_level)?;

    println!("{}", statements.join("\n"));

    return Ok(());
  }
}

/// Helper function
impl DbCli {
  pub fn fetch_snowflake_relation(
    psql: Rc<RefCell<PsqlConnection>>,
    psql_table_by_id: &HashMap<PsqlTableIdentity, PsqlTable>,
    table: &str,
    values: Vec<String>,
    column: &str,
    schema: &str,
  ) -> ResultAnyError<RoseTreeNode<PsqlTableRows>> {
    let table_metadata = Box::new(TableMetadataImpl::new(psql));
    let mut relation_fetcher = psql::relation_fetcher::RelationFetcher::new(table_metadata);

    let input = psql::relation_fetcher::FetchRowsAsRoseTreeInput {
      table_id: &PsqlTableIdentity::new(schema, table),
      column_name: &column,
      column_value: values.get(0).unwrap(), // As of now only supports 1 value
    };

    // As of now only support 1 row
    let tree: RoseTreeNode<PsqlTableRows> = relation_fetcher
      .fetch_rose_trees_to_be_inserted(input, psql_table_by_id)?
      .remove(0);

    return Ok(tree);
  }
}

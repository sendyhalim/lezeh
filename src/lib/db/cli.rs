use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use anyhow::anyhow;
use clap::App as Cli;
use clap::Arg;
use clap::ArgMatches;
use clap::SubCommand;
use petgraph::dot::{Config as GraphDotConfig, Dot as GraphDot};
use petgraph::graph::Graph as BaseGraph;
use petgraph::graph::NodeIndex;
use petgraph::Directed as DirectedGraph;
use slog::Logger;

use crate::common::config::Config;
use crate::common::config::DbConfig;
use crate::common::graph as graph_util;
use crate::common::macros::hashmap_literal;
use crate::common::types::ResultAnyError;
use crate::db::psql;
use crate::db::psql::connection::*;
use crate::db::psql::db_metadata::DbMetadata;
use crate::db::psql::dto::{FromSqlSink, PsqlTable, PsqlTableIdentity, PsqlTableRow};
use crate::db::psql::relation_fetcher::RowGraph;
use crate::db::psql::table_metadata::TableMetadataImpl;

pub struct DbCli {}

enum CherryPickOutputFormatEnum {
  InsertStatement,
  Graphviz,
}

impl From<&str> for CherryPickOutputFormatEnum {
  fn from(s: &str) -> Self {
    match s.to_uppercase().as_ref() {
      "INSERT-STATEMENT" => CherryPickOutputFormatEnum::InsertStatement,
      "GRAPHVIZ" => CherryPickOutputFormatEnum::Graphviz,
      _ => CherryPickOutputFormatEnum::InsertStatement,
    }
  }
}

impl std::fmt::Display for CherryPickOutputFormatEnum {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      CherryPickOutputFormatEnum::InsertStatement => write!(f, "insert-statement"),
      CherryPickOutputFormatEnum::Graphviz => write!(f, "graphviz"),
    }
  }
}

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
              .default_value("public")
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
              .default_value("id")
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
            Arg::with_name("output_format")
              .long("--output-format")
              .required(false)
              .takes_value(true)
              .default_value("insert-statement")
              .possible_values(&["insert-statement", "graphviz"])
              .help("Print format of the cherry pick cli output"),
          )
          .arg(
            Arg::with_name("graph_table_columns")
              .long("--graph-table-columns")
              .required(false)
              .takes_value(true)
              .use_delimiter(true)
              .help("Set the table columns that will be displayed on each node in format '{table_1}:{column_1}|{column_2}|{column_n},{table_n}:{column_n}' for example 'users:id|name|email, orders:|code'"),
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
          .map(ToOwned::to_owned)
          .collect();

        let graph_table_columns: Vec<String> = cherry_pick_cli
          .values_of("graph_table_columns")
          .or_else(|| Some(Default::default()))
          .unwrap()
          .into_iter()
          .map(str::trim)
          .map(ToOwned::to_owned)
          .collect();
        println!("HEHEH {:?}", graph_table_columns);

        return DbCli::cherry_pick(
          cherry_pick_cli.value_of("source_db").unwrap(),
          cherry_pick_cli.value_of("table").unwrap(),
          values,
          cherry_pick_cli.value_of("column").unwrap(),
          cherry_pick_cli.value_of("schema").unwrap(),
          cherry_pick_cli.value_of("output_format").unwrap().into(),
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
    output_format: CherryPickOutputFormatEnum,
    config: Config,
    _logger: Logger,
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

    // --------------------------------

    let (graph, current_node_index) = DbCli::fetch_relation_graph(
      psql.clone(),
      &psql_table_by_id,
      table,
      values,
      column,
      schema,
    )?;

    match output_format {
      CherryPickOutputFormatEnum::InsertStatement => {
        let nodes_by_level = graph_util::create_nodes_by_level(&graph, current_node_index, 0);

        let statements: Vec<String> =
          psql::relation_insert::RelationInsert::into_insert_statements(nodes_by_level)?;
        println!("{}", statements.join("\n"));
      }
      CherryPickOutputFormatEnum::Graphviz => {
        let graph = graph.map(
          |node_index, _node_weight| {
            PsqlTableRowDynamicVisual::new(
              &graph[node_index],
              hashmap_literal! {
                PsqlTableIdentity::new("public", "customers") => vec!["name".to_owned()],
              },
            )
          },
          |edge, _edge_index| edge,
        );

        println!(
          "{:?}",
          GraphDot::with_config(&graph, &[GraphDotConfig::EdgeNoLabel])
        );
      }
    }

    return Ok(());
  }
}

struct PsqlTableRowDynamicVisual<'a> {
  displayed_fields_by_table_id: HashMap<PsqlTableIdentity, Vec<String>>,
  inner: &'a PsqlTableRow,
}

impl<'a> PsqlTableRowDynamicVisual<'a> {
  fn new(
    inner: &'a PsqlTableRow,
    displayed_fields_by_table_id: HashMap<PsqlTableIdentity, Vec<String>>,
  ) -> PsqlTableRowDynamicVisual {
    return PsqlTableRowDynamicVisual {
      displayed_fields_by_table_id,
      inner,
    };
  }
}

impl<'a> std::fmt::Debug for PsqlTableRowDynamicVisual<'a> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    return write!(f, "{}", self as &dyn std::fmt::Display);
  }
}

impl<'a> std::fmt::Display for PsqlTableRowDynamicVisual<'a> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let value_by_column: HashMap<&str, FromSqlSink> = self.inner.get_column_value_map();
    let mut label: String = self.inner.row_id_representation.clone();

    if let Some(fields) = self.displayed_fields_by_table_id.get(&self.inner.table.id) {
      let labels: ResultAnyError<Vec<String>> = fields
        .iter()
        .filter_map(|column_name| {
          return value_by_column.get(&column_name[..]);
        })
        .map(|val| val.to_string_for_statement())
        .collect();

      if labels.is_ok() {
        label = labels
          .unwrap()
          .into_iter()
          .map(|str_val| str_val.trim_matches('\'').to_string())
          .collect::<Vec<String>>()
          .join("\n");
      } else {
        let err = labels.err().unwrap();

        // BAD, but better than letting error goes into limbo since I haven't
        // found way to propagate error properly from anyhow::Error into std::fmt::Error;
        write!(f, "Error when serializing row into string {:?}", err)?;

        // Tell the caller that we have error, the error message is not transmitted though
        // but rather written out directly.
        return Err(std::fmt::Error {});
      }
    }

    return write!(
      f,
      "{}.{} {}",
      self.inner.table.id.schema, self.inner.table.id.name, label
    );
  }
}

/// Helper function
impl DbCli {
  pub fn fetch_relation_graph(
    psql: Rc<RefCell<PsqlConnection>>,
    psql_table_by_id: &HashMap<PsqlTableIdentity, PsqlTable>,
    table: &str,
    values: Vec<String>,
    column: &str,
    schema: &str,
  ) -> ResultAnyError<(RowGraph, NodeIndex)> {
    let table_metadata = Box::new(TableMetadataImpl::new(psql));
    let mut relation_fetcher = psql::relation_fetcher::RelationFetcher::new(table_metadata);

    let input = psql::relation_fetcher::FetchRowsAsRoseTreeInput {
      table_id: &PsqlTableIdentity::new(schema, table),
      column_name: &column,
      column_value: values.get(0).unwrap(), // As of now only supports 1 value
    };

    return relation_fetcher.fetch_as_graphs(input, psql_table_by_id);
  }
}

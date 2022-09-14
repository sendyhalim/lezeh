use std::cell::RefCell;
use std::collections::HashMap;
use std::convert::TryInto;
use std::rc::Rc;

use anyhow::anyhow;
use clap::App as Cli;
use clap::Arg;
use clap::ArgMatches;
use clap::SubCommand;
use lezeh_common::graph as graph_util;
use lezeh_common::types::ResultAnyError;
use petgraph::dot::{Config as GraphDotConfig, Dot as GraphDot};
use petgraph::graph::NodeIndex;
use slog::Logger;

use crate::config::{Config, DbConnectionConfig};
use crate::psql;
use crate::psql::connection::*;
use crate::psql::db_metadata::DbMetadata;
use crate::psql::dto::{FromSqlSink, PsqlTable, PsqlTableIdentity, PsqlTableRow};
use crate::psql::relation_fetcher::RowGraph;
use crate::psql::table_metadata::TableMetadataImpl;

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

  pub fn run(cli: &ArgMatches<'_>, config: Config, logger: &'static Logger) -> ResultAnyError<()> {
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

        return DbCli::cherry_pick(CherryPickInput::new(
          cherry_pick_cli.value_of("source_db").unwrap(),
          cherry_pick_cli.value_of("schema").unwrap(),
          cherry_pick_cli.value_of("table").unwrap(),
          cherry_pick_cli.value_of("column").unwrap(),
          values,
          cherry_pick_cli.value_of("output_format").unwrap().into(),
          graph_table_columns,
          config,
          logger,
        )?);
      }
      _ => Ok(()),
    }
  }
}

struct CherryPickInput<'a> {
  source_db: &'a str,
  schema: &'a str,
  table: &'a str,
  column: &'a str,
  values: Vec<String>,
  output_format: CherryPickOutputFormatEnum,
  displayed_fields_by_table_id: HashMap<PsqlTableIdentity, Vec<String>>,
  config: Config,
  logger: &'static Logger,
}

impl<'a> CherryPickInput<'a> {
  pub fn new(
    source_db: &'a str,
    schema: &'a str,
    table: &'a str,
    column: &'a str,
    values: Vec<String>,
    output_format: CherryPickOutputFormatEnum,
    graph_table_columns: Vec<String>,
    config: Config,
    logger: &'static Logger,
  ) -> ResultAnyError<CherryPickInput<'a>> {
    return Ok(CherryPickInput {
      source_db,
      schema,
      table,
      values,
      column,
      output_format,
      displayed_fields_by_table_id:
        CherryPickInput::create_displayed_fields_by_table_id_from_param(graph_table_columns)?,
      config,
      logger,
    });
  }

  fn create_displayed_fields_by_table_id_from_param(
    graph_table_columns: Vec<String>,
  ) -> ResultAnyError<HashMap<PsqlTableIdentity, Vec<String>>> {
    return graph_table_columns
      .into_iter()
      .map(|displayed_table_column_str| {
        let (table_id_str, pipe_separated_column) =
          displayed_table_column_str.split_once(':').ok_or_else(|| {
            return anyhow!(
              "Display table columsn should be in format {{tableIdentity}}:{{column_1}}|{{column_n}}, got {} instead",
              displayed_table_column_str
            );
          })?;

        return Ok(table_id_str.try_into().map(|table_id| {
          (
            table_id,
            pipe_separated_column
              .split('|')
              .into_iter()
              .map(str::trim)
              .map(ToOwned::to_owned)
              .collect(),
          )
        }));
      })
      .collect::<ResultAnyError<Vec<_>>>()?
      .into_iter()
      .collect();
  }
}

/// 1 method represents 1 CLI command
impl DbCli {
  fn cherry_pick<'a>(input: CherryPickInput) -> ResultAnyError<()> {
    let CherryPickInput {
      source_db,
      schema,
      table,
      values,
      column,
      output_format,
      displayed_fields_by_table_id,
      config,
      logger,
    } = input;

    let source_db_config: DbConnectionConfig = config
      .db_connection_by_name
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
            PsqlTableRowDynamicVisual::new(&graph[node_index], &displayed_fields_by_table_id)
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
  displayed_fields_by_table_id: &'a HashMap<PsqlTableIdentity, Vec<String>>,
  inner: &'a PsqlTableRow,
}

impl<'a> PsqlTableRowDynamicVisual<'a> {
  fn new(
    inner: &'a PsqlTableRow,
    displayed_fields_by_table_id: &'a HashMap<PsqlTableIdentity, Vec<String>>,
  ) -> PsqlTableRowDynamicVisual<'a> {
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
    let mut label: String = format!("`id` {}", self.inner.row_id_representation);

    if let Some(fields) = self.displayed_fields_by_table_id.get(&self.inner.table.id) {
      let labels: ResultAnyError<Vec<(String, String)>> = fields
        .iter()
        .filter_map(|column_name| {
          return value_by_column
            .get(&column_name[..])
            .map(|val| (column_name.clone(), val));
        })
        .map(|(column_name, val)| {
          val
            .to_string_for_statement()
            .map(|str_val| (column_name, str_val))
        })
        .collect();

      if labels.is_ok() {
        label = labels
          .unwrap()
          .into_iter()
          .map(|(column_name, str_val)| {
            format!(
              "`{}` {}",
              column_name,
              str_val.trim_matches('\'').to_string()
            )
          })
          .collect::<Vec<String>>()
          .join("\n");
      } else {
        let err = labels.err().unwrap();

        // BAD, but better than letting error goes into limbo since I haven't
        // found a way to propagate error properly from anyhow::Error into std::fmt::Error;
        write!(f, "Error when serializing row into string {:?}", err)?;

        // Tell the caller that we have error, the error message is not transmitted though
        // but rather written out directly.
        return Err(std::fmt::Error {});
      }
    }

    return write!(f, "{}\n{}", self.inner.table.id, label);
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

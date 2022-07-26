use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use itertools::Itertools;
use postgres::row::Row;
use postgres::Column;

use crate::common::types::ResultAnyError;
use crate::db::psql::dto::FromSqlSink;
use crate::db::psql::dto::PsqlTable;
use crate::db::psql::dto::PsqlTableIdentity;
use crate::db::psql::dto::PsqlTableRows;

pub struct TableInsertStatement<'a> {
  table: PsqlTable,
  columns: TableInsertRowColumns<'a>,
  row_values: Vec<TableInsertRowValues>,
}

impl<'a> std::fmt::Display for TableInsertStatement<'a> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    // let template = ;

    return write!(
      f,
      indoc::indoc! {"
        ------------------------------------------------
        -- insert into table {}
        ------------------------------------------------
        insert into {} ({}) VALUES
          {};
        ---------------

      "},
      self.table.id,
      self.table.id,
      self.columns,
      self
        .row_values
        .iter()
        .map(|val| format!("{}", val))
        .collect::<Vec<String>>()
        .join(",\n"),
    );
  }
}

pub struct TableInsertRowColumns<'a> {
  columns: &'a [Column],
}

impl<'a> std::fmt::Display for TableInsertRowColumns<'a> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let column_string: String = self
      .columns
      .iter()
      .map(|c| String::from(c.name()))
      .collect::<Vec<String>>()
      .join(", ");

    return write!(f, "{}", column_string);
  }
}

pub struct TableInsertRowValues {
  values: Vec<String>,
}

impl std::fmt::Display for TableInsertRowValues {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    return write!(f, "({})", self.values.join(", "));
  }
}

pub struct RelationInsert {}

impl RelationInsert {
  pub fn into_insert_statements(
    rows_by_level: HashMap<i32, HashSet<PsqlTableRows>>,
  ) -> ResultAnyError<Vec<String>> {
    let mut levels: Vec<&i32> = rows_by_level.keys().collect();

    levels.sort();

    let insert_statements: ResultAnyError<Vec<Vec<String>>> = levels
      .iter()
      .map(|level| {
        let rows: &HashSet<PsqlTableRows> = rows_by_level.get(level).unwrap();

        return RelationInsert::table_rows_into_insert_statement(rows);
      })
      .collect();

    return Ok(insert_statements?.into_iter().flatten().collect());
  }

  pub fn table_rows_into_insert_statement(
    rows: &HashSet<PsqlTableRows>,
  ) -> ResultAnyError<Vec<String>> {
    // Rows of the same table can be scattered through vec of psql table rows,
    // remember Vec<PsqlTableRows> meaning Vec<Vec<Row>> due to PsqlTableRows
    // contains `rows: Vec<Row>`. So here we're trying to group
    // scattered rows by table id
    let psql_table_by_id: HashMap<PsqlTableIdentity, PsqlTable> = rows
      .iter()
      .map(|row| (row.table.id.clone(), row.table.clone()))
      .collect();

    let psql_rows_by_table_id: HashMap<PsqlTableIdentity, Vec<Vec<Rc<Row>>>> = rows
      .iter()
      .map(|psql_table_row| (psql_table_row.table.id.clone(), psql_table_row.rows.clone()))
      .into_group_map();

    let rows_by_table_id: HashMap<PsqlTableIdentity, Vec<Rc<Row>>> = psql_rows_by_table_id
      .into_iter()
      .map(
        |(table_identity, rows_2d): (PsqlTableIdentity, Vec<Vec<Rc<Row>>>)| {
          return (table_identity, rows_2d.into_iter().flatten().collect_vec());
        },
      )
      .collect();

    return rows_by_table_id
      .iter()
      .map(|(table_id, rows)| {
        return RelationInsert::table_row_into_insert_statement(
          psql_table_by_id.get(table_id).unwrap(),
          rows,
        );
      })
      .collect::<ResultAnyError<Vec<String>>>();
  }

  pub fn table_row_into_insert_statement(
    table: &PsqlTable,
    rows: &Vec<Rc<Row>>,
  ) -> ResultAnyError<String> {
    let first_row: Rc<Row> = rows.get(0).unwrap().clone();
    let table_insert_row_columns = TableInsertRowColumns {
      columns: first_row.columns(),
    };

    let row_values: Vec<TableInsertRowValues> = rows
      .iter()
      .map(|row| {
        return table_insert_row_columns
          .columns
          .iter()
          .map(|c| {
            let sink = row.get::<'_, _, FromSqlSink>(c.name());

            return sink.to_string_for_statement();
          })
          .collect::<ResultAnyError<Vec<String>>>()
          .map(|values_in_string| {
            return TableInsertRowValues {
              values: values_in_string,
            };
          });
      })
      .collect::<ResultAnyError<Vec<TableInsertRowValues>>>()?;

    let table_insert_statement = TableInsertStatement {
      table: table.clone(),
      columns: table_insert_row_columns,
      row_values,
    };

    return Ok(format!("{}", table_insert_statement));
  }
}

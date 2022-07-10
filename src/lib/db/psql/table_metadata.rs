use std::cell::RefCell;
use std::rc::Rc;

use anyhow::anyhow;
use postgres::types::ToSql;
use postgres::Row;
use thiserror::Error;

use crate::common::types::ResultAnyError;
use crate::db::psql::connection::PsqlConnection;
use crate::db::psql::dto::*;

pub type PsqlParamValue = Box<dyn ToSql + Sync>;

pub struct Query {
  connection: Rc<RefCell<PsqlConnection>>,
}

#[derive(Error, Debug)]
pub enum QueryError {
  #[error("Row with column {column} = {identifier:?} is not found in table {table_name}")]
  RowNotFound {
    table_name: String,
    column: String,
    identifier: String,
  },

  #[error("Too many rows returned({row_count}), expecting only {expected_row_count}")]
  TooManyRows {
    row_count: usize,
    expected_row_count: usize,
  },
}

pub struct FetchRowInput<'a> {
  pub schema: &'a str,
  pub table_name: &'a str,
  pub column_name: &'a str,
  pub column_value: &'a PsqlParamValue,
}

impl<'b> FetchRowInput<'b> {
  pub fn psql_param_value<'a>(
    column_value: String,
    column: PsqlTableColumn<'a>,
  ) -> ResultAnyError<PsqlParamValue> {
    let data_type: String = column.data_type.to_string();
    let mut value: PsqlParamValue = Box::new(column_value.clone());

    if data_type == "integer" {
      value = Box::new(column_value.clone().parse::<i32>()?);
    } else if data_type == "uuid" {
      let uuid = Uuid::from_str(&column_value)?;

      value = Box::new(uuid);
    }

    return Ok(value);
  }
}

impl Query {
  fn find_rows(&mut self, input: &FetchRowInput) -> ResultAnyError<Vec<Row>> {
    let query_str = format!(
      "SELECT * FROM {} where {} = $1",
      input.table_name, input.column_name
    );

    let mut connection = self.connection.borrow_mut();
    let connection = connection.get();
    let statement = connection.prepare(&query_str)?;

    return connection
      .query(&statement, &[input.column_value.as_ref()])
      .map_err(anyhow::Error::from);
  }

  fn find_one_row(&mut self, input: &FetchRowInput) -> ResultAnyError<Option<Row>> {
    let rows_result = self.find_rows(input);

    return match rows_result {
      Err(any) => Err(any),
      Ok(mut rows) => {
        if rows.len() > 1 {
          return Err(anyhow!(QueryError::TooManyRows {
            row_count: rows.len(),
            expected_row_count: 1,
          }));
        }

        if rows.len() == 0 {
          return Ok(None);
        }

        return Ok(Some(rows.remove(0)));
      }
    };
  }

  pub fn get_column_metadata(
    &mut self,
    schema: &str,
    table_name: &str,
    column_name: &str,
  ) -> ResultAnyError<Row> {
    let query_str =
      "SELECT * FROM information_schema.columns where table_schema = $1 and table_name = $2 and column_name = $3";

    let mut connection = self.connection.borrow_mut();
    let connection = connection.get();
    let statement = connection.prepare(&query_str)?;

    return connection
      .query_one(
        &statement,
        &[
          &schema.to_string(),
          &table_name.to_string(),
          &column_name.to_string(),
        ],
      )
      .map_err(anyhow::Error::from);
  }
}

pub struct TableMetadata {
  /// We know that we own this query so it's ok
  /// to directl borrow_mut() without checking ownership
  query: RefCell<Query>,
}

impl TableMetadata {
  pub fn new(psql_connection: Rc<RefCell<PsqlConnection>>) -> TableMetadata {
    return TableMetadata {
      query: RefCell::new(Query {
        connection: psql_connection,
      }),
    };
  }
}

impl TableMetadata {
  pub fn get_column(
    &self,
    schema: &str,
    table_name: &str,
    column_name: &str,
  ) -> ResultAnyError<PsqlTableColumn> {
    let row = self
      .query
      .borrow_mut()
      .get_column_metadata(schema, table_name, column_name)?;

    let column = PsqlTableColumn::new(column_name.to_string(), row.get("data_type"));

    return Ok(column);
  }

  pub fn get_psql_table_rows<'a>(
    &self,
    table: PsqlTable<'a>,
    column_name: &str,
    id: &PsqlParamValue,
  ) -> ResultAnyError<PsqlTableRows<'a>> {
    let rows = self.query.borrow_mut().find_rows(&FetchRowInput {
      schema: table.schema.as_ref(),
      table_name: table.name.as_ref(),
      column_name,
      column_value: id,
    })?;

    return Ok(PsqlTableRows {
      table: table.clone(),
      rows: rows.into_iter().map(Rc::new).collect(),
    });
  }

  pub fn get_one_row<'a>(
    &self,
    table: &PsqlTable<'a>,
    column_name: &str,
    id: &str,
  ) -> ResultAnyError<Row> {
    let column = self.get_column(&table.schema, &table.name, column_name)?;
    let id: PsqlParamValue = FetchRowInput::psql_param_value(id.to_string(), column)?;

    let row = self.query.borrow_mut().find_one_row(&FetchRowInput {
      schema: table.schema.as_ref(),
      table_name: table.name.as_ref(),
      column_name,
      column_value: &id,
    })?;

    return row.ok_or_else(|| {
      anyhow!(QueryError::RowNotFound {
        table_name: table.name.to_string(),
        column: column_name.into(),
        identifier: format!("{:#?}", id),
      })
    });
  }
}

pub struct RowUtil;

impl RowUtil {
  pub fn get_id_from_row(row: &Row, id_column_spec: &PsqlTableColumn) -> PsqlParamValue {
    if id_column_spec.data_type == "integer" {
      return Box::new(row.get::<_, i32>(id_column_spec.name.as_ref()));
    }

    if id_column_spec.data_type == "uuid" {
      return Box::new(row.get::<_, Uuid>(id_column_spec.name.as_ref()));
    }

    return Box::new(row.get::<_, String>(id_column_spec.name.as_ref()));
  }
}

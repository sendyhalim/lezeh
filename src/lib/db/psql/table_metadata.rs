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
  #[error("Row with column {column} = {identifier:?} is not found in table {table_id}")]
  RowNotFound {
    table_id: String,
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
  pub table_id: &'a PsqlTableIdentity,
  pub column_name: &'a str,
  pub column_value: &'a PsqlParamValue,
}

impl<'b> FetchRowInput<'b> {
  pub fn psql_param_value<'a>(
    column_value: String,
    column: PsqlTableColumn,
  ) -> ResultAnyError<PsqlParamValue> {
    let data_type: String = column.data_type.to_string();
    let mut value: PsqlParamValue = Box::new(column_value.clone());

    if data_type == "integer" {
      let convert_column_value = column_value.clone().parse::<i32>().map_err(|err| {
        return anyhow!(
          "Cannot cast column '{}' of value {} to integer. Error: {}",
          column.name,
          column_value,
          err
        );
      })?;

      value = Box::new(convert_column_value);
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
      input.table_id, input.column_name
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

  pub fn get_column_metadata<'a>(
    &mut self,
    table_id: &PsqlTableIdentity,
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
          &table_id.schema.to_string(),
          &table_id.name.to_string(),
          &column_name.to_string(),
        ],
      )
      .map_err(anyhow::Error::from);
  }
}

#[cfg_attr(test, mockall::automock)]
pub trait TableMetadata {
  fn get_column(
    &self,
    table_id: &PsqlTableIdentity,
    column_name: &str,
  ) -> ResultAnyError<PsqlTableColumn>;

  fn get_rows<'a>(
    &self,
    table: PsqlTable,
    column_name: &str,
    id: &PsqlParamValue,
  ) -> ResultAnyError<Vec<Row>>;

  fn get_one_row(&self, table: &PsqlTable, column_name: &str, id: &str) -> ResultAnyError<Row>;
}

pub struct TableMetadataImpl {
  /// We know that we own this query so it's ok
  /// to directl borrow_mut() without checking ownership
  query: RefCell<Query>,
}

impl TableMetadataImpl {
  pub fn new(psql_connection: Rc<RefCell<PsqlConnection>>) -> TableMetadataImpl {
    return TableMetadataImpl {
      query: RefCell::new(Query {
        connection: psql_connection,
      }),
    };
  }
}

impl TableMetadata for TableMetadataImpl {
  fn get_column(
    &self,
    table_id: &PsqlTableIdentity,
    column_name: &str,
  ) -> ResultAnyError<PsqlTableColumn> {
    let row = self
      .query
      .borrow_mut()
      .get_column_metadata(table_id, column_name)?;

    let column = PsqlTableColumn::new(column_name.to_string(), row.get("data_type"));

    return Ok(column);
  }

  fn get_rows(
    &self,
    table: PsqlTable,
    column_name: &str,
    id: &PsqlParamValue,
  ) -> ResultAnyError<Vec<Row>> {
    return self.query.borrow_mut().find_rows(&FetchRowInput {
      table_id: &table.id,
      column_name,
      column_value: id,
    });
  }

  fn get_one_row<'a>(&self, table: &PsqlTable, column_name: &str, id: &str) -> ResultAnyError<Row> {
    let column = self.get_column(&table.id, column_name)?;
    let id: PsqlParamValue = FetchRowInput::psql_param_value(id.to_string(), column)?;

    let row = self.query.borrow_mut().find_one_row(&FetchRowInput {
      table_id: &table.id,
      column_name,
      column_value: &id,
    })?;

    return row.ok_or_else(|| {
      anyhow!(QueryError::RowNotFound {
        table_id: format!("{:#?}", table.id),
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
      return Box::new(row.get::<_, i32>(id_column_spec.name.as_str()));
    }

    if id_column_spec.data_type == "uuid" {
      return Box::new(row.get::<_, Uuid>(id_column_spec.name.as_str()));
    }

    return Box::new(row.get::<_, String>(id_column_spec.name.as_str()));
  }
}

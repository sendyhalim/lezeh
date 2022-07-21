use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use chrono::{NaiveDate, NaiveDateTime};
use postgres::row::Row;
use postgres::types::FromSql;
use postgres::Column;
use postgres_types::Type as PsqlType;

use crate::common::types::ResultAnyError;
use crate::db::psql::dto::PsqlTable;
use crate::db::psql::dto::PsqlTableRows;
use crate::db::psql::dto::Uuid;

pub struct TableInsertStatement<'a> {
  table: PsqlTable<'a>,
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
    return rows
      .iter()
      .map(|psql_table_row| {
        return RelationInsert::table_row_into_insert_statement(psql_table_row);
      })
      .collect::<ResultAnyError<Vec<String>>>();
  }

  pub fn table_row_into_insert_statement(table_row: &PsqlTableRows) -> ResultAnyError<String> {
    let rows: &Vec<Rc<Row>> = &table_row.rows;
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
      table: table_row.table.clone(),
      columns: table_insert_row_columns,
      row_values,
    };

    return Ok(format!("{}", table_insert_statement));
  }
}

/// Structure that act as a sink to drain bytes
/// from postgres::row::Row
struct FromSqlSink {
  raw: Vec<u8>,
  ty: Option<postgres::types::Type>, // None if null
}

impl<'a> FromSql<'a> for FromSqlSink {
  fn from_sql(
    ty: &PsqlType,
    raw: &'a [u8],
  ) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
    let sink = FromSqlSink {
      raw: raw.to_owned(),
      ty: Some(ty.to_owned()),
    };

    return Ok(sink);
  }

  fn from_sql_null(_ty: &PsqlType) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
    return Ok(FromSqlSink {
      raw: vec![],
      ty: None,
    });
  }

  fn accepts(_ty: &PsqlType) -> bool {
    return true;
  }
}

impl FromSqlSink {
  pub fn escape_string<T>(val: T) -> String
  where
    T: ToString,
  {
    // https://github.com/sfackler/rust-postgres/pull/702
    return postgres_protocol::escape::escape_literal(&val.to_string());
  }

  pub fn to_string_for_statement(&self) -> ResultAnyError<String> {
    if self.ty.is_none() {
      return Ok("null".into());
    }

    let ty: &PsqlType = self.ty.as_ref().unwrap();

    return match *ty {
      PsqlType::BOOL => postgres_protocol::types::bool_from_sql(&self.raw[..])
        .map(|val| val.to_string())
        .map_err(anyhow::Error::msg),

      PsqlType::INT4 => postgres_protocol::types::int4_from_sql(&self.raw[..])
        .map(|val| val.to_string())
        .map_err(anyhow::Error::msg),

      PsqlType::INT2 => postgres_protocol::types::int2_from_sql(&self.raw[..])
        .map(|val| val.to_string())
        .map_err(anyhow::Error::msg),

      PsqlType::INT8 => postgres_protocol::types::int8_from_sql(&self.raw[..])
        .map(|val| val.to_string())
        .map_err(anyhow::Error::msg),

      // https://github.com/sfackler/rust-postgres/blob/master/postgres-types/src/chrono_04.rs
      PsqlType::DATE => {
        return NaiveDate::from_sql(ty, &self.raw[..])
          .map(FromSqlSink::escape_string)
          .map_err(anyhow::Error::msg);
      }

      PsqlType::TIMESTAMP | PsqlType::TIMESTAMPTZ => {
        return NaiveDateTime::from_sql(ty, &self.raw[..])
          .map(FromSqlSink::escape_string)
          .map_err(anyhow::Error::msg);
      }

      PsqlType::NUMERIC => rust_decimal::Decimal::from_sql(&ty, &self.raw)
        .map(|val| val.to_string())
        .map_err(anyhow::Error::msg),

      PsqlType::UUID => {
        return Uuid::from_sql(ty, &self.raw)
          .map(|val| {
            return format!("'{}'", val.to_string());
          })
          .map_err(anyhow::Error::msg);
      }

      _ => postgres_protocol::types::text_from_sql(&self.raw[..])
        .map(FromSqlSink::escape_string)
        .map_err(anyhow::Error::msg),
    };
  }
}

use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::Hash;
use std::rc::Rc;

use chrono::{NaiveDate, NaiveDateTime};
use postgres::types::to_sql_checked;
use postgres::types::FromSql;
use postgres::types::ToSql;
use postgres::Row;
use postgres_types::Type as PsqlType;

use crate::common::types::ResultAnyError;

type AnyString<'a> = Cow<'a, str>;
pub type PsqlParamValue = Box<dyn ToSql + Sync>;

#[derive(PartialEq, Hash, Eq, Debug, Clone)]
pub struct PsqlTableColumn {
  pub name: String,
  pub data_type: String,
}

impl PsqlTableColumn {
  pub fn new<'a, S>(name: S, data_type: S) -> PsqlTableColumn
  where
    S: Into<AnyString<'a>>,
  {
    return PsqlTableColumn {
      name: name.into().to_string(),
      data_type: data_type.into().to_string(),
    };
  }
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct PsqlForeignKey {
  pub name: String,
  pub column: PsqlTableColumn,
  pub foreign_table_schema: String,
  pub foreign_table_name: String,
}

impl PsqlForeignKey {
  pub fn new<'a, S>(
    name: S,
    column: PsqlTableColumn,
    foreign_table_schema: S,
    foreign_table_name: S,
  ) -> PsqlForeignKey
  where
    S: Into<AnyString<'a>>,
  {
    return PsqlForeignKey {
      name: name.into().to_string(),
      column,
      foreign_table_schema: foreign_table_schema.into().to_string(),
      foreign_table_name: foreign_table_name.into().to_string(),
    };
  }
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct PsqlTableIdentity {
  pub schema: String,
  pub name: String,
}

impl PsqlTableIdentity {
  pub fn new<'a, S>(schema: S, name: S) -> PsqlTableIdentity
  where
    S: Into<AnyString<'a>>,
  {
    return PsqlTableIdentity {
      schema: schema.into().to_string(),
      name: name.into().to_string(),
    };
  }
}

impl std::fmt::Display for PsqlTableIdentity {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    return write!(f, "{}.{}", self.schema, self.name);
  }
}

impl Hash for PsqlTableIdentity {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    self.schema.hash(state);
    self.name.hash(state);
  }
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct PsqlTable {
  pub id: PsqlTableIdentity,
  pub primary_column: PsqlTableColumn,
  pub columns: HashSet<PsqlTableColumn>,
  pub referenced_fk_by_constraint_name: HashMap<String, PsqlForeignKey>,
  pub referencing_fk_by_constraint_name: HashMap<String, PsqlForeignKey>,
}

impl PsqlTable {
  pub fn new<'a, S>(
    schema: S,
    name: S,
    primary_column: PsqlTableColumn,
    columns: HashSet<PsqlTableColumn>,
    referenced_fk_by_constraint_name: HashMap<String, PsqlForeignKey>,
    referencing_fk_by_constraint_name: HashMap<String, PsqlForeignKey>,
  ) -> PsqlTable
  where
    S: Into<AnyString<'a>>,
  {
    return PsqlTable {
      id: PsqlTableIdentity::new(schema, name),
      primary_column,
      columns,
      referenced_fk_by_constraint_name,
      referencing_fk_by_constraint_name,
    };
  }
}

#[derive(Debug, Clone)]
pub struct PsqlTableRow {
  pub table: PsqlTable,
  pub row_id_representation: String,
  inner_row: Rc<Row>,
}

impl PsqlTableRow {
  pub fn new(table: PsqlTable, row: Rc<Row>) -> PsqlTableRow {
    let sink = row.get::<'_, _, FromSqlSink>("id");

    // TODO: NOT GOOD, find better ways
    let row_id = sink.to_string_for_statement().unwrap();

    return PsqlTableRow {
      table,
      row_id_representation: row_id,
      inner_row: row,
    };
  }
}

impl PsqlTableRow {
  pub fn get_id(&self, id_column_spec: &PsqlTableColumn) -> PsqlParamValue {
    let inner_row = &self.inner_row;

    if id_column_spec.data_type == "integer" {
      return Box::new(inner_row.get::<_, i32>(id_column_spec.name.as_str()));
    }

    if id_column_spec.data_type == "uuid" {
      return Box::new(inner_row.get::<_, Uuid>(id_column_spec.name.as_str()));
    }

    return Box::new(inner_row.get::<_, String>(id_column_spec.name.as_str()));
  }

  pub fn get_column_value_map<'a, T>(&'a self) -> HashMap<String, T>
  where
    T: FromSql<'a>,
  {
    return self
      .inner_row
      .columns()
      .iter()
      .map(|c| {
        let val = self.inner_row.get::<'a, _, T>(c.name());

        return (c.name().to_string(), val);
      })
      .collect();
  }
}

impl PartialEq for PsqlTableRow {
  fn eq(&self, other: &Self) -> bool {
    return self.table == other.table && self.row_id_representation == other.row_id_representation;
  }
}

impl Eq for PsqlTableRow {}

impl Hash for PsqlTableRow {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    self.table.id.hash(state);
    self.row_id_representation.hash(state);

    // println!("hash {} {}", self.table.id, id);
  }
}

#[derive(Debug)]
pub struct Uuid {
  bytes: [u8; 16],
}

impl Uuid {
  pub fn from_bytes(bytes: [u8; 16]) -> Self {
    return Uuid { bytes };
  }

  pub fn from_str(val: &str) -> ResultAnyError<Self> {
    // Use uuid::* package to ease some uuid operations
    return Ok(Uuid::from_bytes(*uuid::Uuid::parse_str(val)?.as_bytes()));
  }
}

impl ToSql for Uuid {
  fn to_sql(
    &self,
    _ty: &postgres_types::Type,
    out: &mut postgres_types::private::BytesMut,
  ) -> Result<postgres_types::IsNull, Box<dyn std::error::Error + Sync + Send>>
  where
    Self: Sized,
  {
    out.extend_from_slice(&self.bytes);

    return Ok(postgres_types::IsNull::No);
  }

  fn accepts(ty: &postgres_types::Type) -> bool
  where
    Self: Sized,
  {
    return *ty == postgres_types::Type::UUID;
  }

  to_sql_checked!();
}

impl<'a> FromSql<'a> for Uuid {
  fn from_sql(
    _ty: &postgres_types::Type,
    raw: &'a [u8],
  ) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
    let mut bytes: [u8; 16] = [0; 16];

    bytes.clone_from_slice(raw);

    return Ok(Uuid { bytes });
  }

  fn accepts(ty: &postgres_types::Type) -> bool {
    return *ty == postgres_types::Type::UUID;
  }
}

impl ToString for Uuid {
  fn to_string(&self) -> String {
    return uuid::Builder::from_bytes(self.bytes)
      .into_uuid()
      .to_string();
    // return String::from_utf8_lossy(&self.bytes).to_string();
  }
}

/// Structure that act as a sink to drain bytes
/// from postgres::row::Row
pub struct FromSqlSink {
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

#[cfg(test)]
mod test {
  use super::*;

  mod uuid {
    use super::*;

    mod from_str {
      use super::*;

      #[test]
      fn it_should_create_uuid_instance() -> ResultAnyError<()> {
        let uuid_str: &str = "83166f85-d37a-4fe7-a0f6-ad5103d03f8a";

        let parsed: ResultAnyError<Uuid> = Uuid::from_str(uuid_str);

        assert!(parsed.is_ok());
        assert_eq!(parsed?.to_string(), uuid_str);

        return Ok(());
      }
    }

    mod from_sql {
      use super::*;

      #[test]
      fn it_should_create_from_sql_bytes() -> Result<(), Box<dyn std::error::Error + Sync + Send>> {
        let ty: postgres_types::Type = postgres_types::Type::UUID;

        // Online uuid to bytes converter:
        // https://yupana-engineering.com/online-uuid-to-c-array-converter
        let bytes: Vec<u8> = vec![
          0x83, 0x16, 0x6f, 0x85, 0xd3, 0x7a, 0x4f, 0xe7, 0xa0, 0xf6, 0xad, 0x51, 0x03, 0xd0, 0x3f,
          0x8a,
        ];

        let parsed_uuid: Uuid = Uuid::from_sql(&ty, &bytes)?;

        assert_eq!(
          parsed_uuid.to_string(),
          "83166f85-d37a-4fe7-a0f6-ad5103d03f8a"
        );

        return Ok(());
      }
    }
  }
}

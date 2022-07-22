use std::borrow::Cow;
use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::Hash;
use std::rc::Rc;

use postgres::types::to_sql_checked;
use postgres::types::FromSql;
use postgres::types::ToSql;
use postgres::Row;

use crate::common::types::ResultAnyError;

type AnyString<'a> = Cow<'a, str>;

#[derive(PartialEq, Hash, Eq, Debug, Clone)]
pub struct PsqlTableColumn<'a> {
  pub name: AnyString<'a>,
  pub data_type: AnyString<'a>,
}

impl<'a> PsqlTableColumn<'a> {
  pub fn new<S>(name: S, data_type: S) -> PsqlTableColumn<'a>
  where
    S: Into<AnyString<'a>>,
  {
    return PsqlTableColumn {
      name: name.into(),
      data_type: data_type.into(),
    };
  }
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct PsqlForeignKey<'a> {
  pub name: AnyString<'a>,
  pub column: PsqlTableColumn<'a>,
  pub foreign_table_schema: AnyString<'a>,
  pub foreign_table_name: AnyString<'a>,
}

impl<'a> PsqlForeignKey<'a> {
  pub fn new<S>(
    name: S,
    column: PsqlTableColumn<'a>,
    foreign_table_schema: S,
    foreign_table_name: S,
  ) -> PsqlForeignKey<'a>
  where
    S: Into<AnyString<'a>>,
  {
    return PsqlForeignKey {
      name: name.into(),
      column,
      foreign_table_schema: foreign_table_schema.into(),
      foreign_table_name: foreign_table_name.into(),
    };
  }
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct PsqlTableIdentity<'a> {
  pub schema: AnyString<'a>,
  pub name: AnyString<'a>,
}

impl<'a> PsqlTableIdentity<'a> {
  pub fn new<S>(schema: S, name: S) -> PsqlTableIdentity<'a>
  where
    S: Into<AnyString<'a>>,
  {
    return PsqlTableIdentity {
      schema: schema.into(),
      name: name.into(),
    };
  }
}

impl std::fmt::Display for PsqlTableIdentity<'_> {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    return write!(f, "{}.{}", self.schema, self.name);
  }
}

impl<'a> Hash for PsqlTableIdentity<'a> {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    self.schema.hash(state);
    self.name.hash(state);
  }
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct PsqlTable<'a> {
  pub id: PsqlTableIdentity<'a>,
  pub primary_column: PsqlTableColumn<'a>,
  pub columns: HashSet<PsqlTableColumn<'a>>,
  pub referenced_fk_by_constraint_name: HashMap<String, PsqlForeignKey<'a>>,
  pub referencing_fk_by_constraint_name: HashMap<String, PsqlForeignKey<'a>>,
}

impl<'a> PsqlTable<'a> {
  pub fn new<S>(
    schema: S,
    name: S,
    primary_column: PsqlTableColumn<'a>,
    columns: HashSet<PsqlTableColumn<'a>>,
    referenced_fk_by_constraint_name: HashMap<String, PsqlForeignKey<'a>>,
    referencing_fk_by_constraint_name: HashMap<String, PsqlForeignKey<'a>>,
  ) -> PsqlTable<'a>
  where
    S: Into<AnyString<'a>>,
  {
    return PsqlTable {
      id: PsqlTableIdentity {
        schema: schema.into(),
        name: name.into(),
      },
      primary_column,
      columns,
      referenced_fk_by_constraint_name,
      referencing_fk_by_constraint_name,
    };
  }
}

#[derive(Debug, Clone)]
pub struct PsqlTableRows<'a> {
  pub table: PsqlTable<'a>,
  pub rows: Vec<Rc<Row>>,
}

impl<'a> PartialEq for PsqlTableRows<'a> {
  fn eq(&self, other: &Self) -> bool {
    return self.table == other.table;
  }
}

impl<'a> Eq for PsqlTableRows<'a> {}

impl<'a> Hash for PsqlTableRows<'a> {
  fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
    self.table.id.hash(state);
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

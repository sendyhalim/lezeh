use crate::db::psql::dto::PsqlTableRows;
use postgres::row::Row;
use postgres::types::FromSql;
use postgres::Column;
use postgres_types::Type as PsqlType;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

pub struct RelationInsert {}

impl RelationInsert {
  pub fn into_insert_statements(
    rows_by_level: HashMap<i32, HashSet<PsqlTableRows>>,
  ) -> Vec<String> {
    let levels: Vec<&i32> = rows_by_level.keys().collect();

    let statements: Vec<String> = levels
      .iter()
      .flat_map(|level| {
        let rows: &HashSet<PsqlTableRows> = rows_by_level.get(level).unwrap();

        return rows
          .iter()
          .map(|psql_table_row| {
            return RelationInsert::table_row_into_insert_statement(psql_table_row);
          })
          .collect::<Vec<String>>();
      })
      .collect();

    return statements;
  }

  pub fn table_row_into_insert_statement(table_row: &PsqlTableRows) -> String {
    let rows: &Vec<Rc<Row>> = &table_row.rows;
    let first_row: Rc<Row> = rows.get(0).unwrap().clone();
    let columns: &[Column] = first_row.columns(); // Slice
    let column_string: String = columns
      .iter()
      .map(|c| String::from(c.name()))
      .collect::<Vec<String>>()
      .join(",");

    let values: String = rows
      .iter()
      .map(|row| {
        let row_values_str = columns
          .iter()
          .map(|c| {
            let sink: FromSqlSink = row.get::<'_, _, FromSqlSink>(c.name());

            return format!("{}", sink.to_string_for_statement());
          })
          .collect::<Vec<String>>()
          .join(",");

        return format!("({})", row_values_str);
      })
      .collect::<Vec<String>>()
      .join(",");

    return format!(
      "insert into {} ({}) VALUES {}",
      table_row.table.name, column_string, values
    );
  }
}

/// Structure that act as a sink to drain bytes
/// from postgres::row::Row
struct FromSqlSink {
  raw: Vec<u8>,
  ty: postgres::types::Type,
}

impl<'a> FromSql<'a> for FromSqlSink {
  fn from_sql(
    ty: &PsqlType,
    raw: &'a [u8],
  ) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
    let sink = FromSqlSink {
      raw: raw.to_owned(),
      ty: ty.to_owned(),
    };

    return Ok(sink);
  }

  fn accepts(ty: &PsqlType) -> bool {
    return true;
  }
}

impl FromSqlSink {
  pub fn enclosed_statement_string_value(&self, enclosing_val: &str) -> String {
    println!("STRING VALUE OF type {} {:#?}", self.ty, self.raw);
    return format!(
      "{}{}{}",
      enclosing_val,
      postgres_protocol::types::text_from_sql(&self.raw[..]).unwrap(),
      enclosing_val,
    );
  }

  pub fn to_string_for_statement(&self) -> String {
    // let ty = &self.ty;

    return match self.ty {
      PsqlType::VARCHAR
      | PsqlType::TEXT
      | PsqlType::BPCHAR
      | PsqlType::NAME
      | PsqlType::UNKNOWN => self.enclosed_statement_string_value("'"),

      PsqlType::BOOL => postgres_protocol::types::bool_from_sql(&self.raw[..])
        .unwrap()
        .to_string(),

      PsqlType::INT4 => postgres_protocol::types::int4_from_sql(&self.raw[..])
        .unwrap()
        .to_string(),

      PsqlType::INT2 => postgres_protocol::types::int2_from_sql(&self.raw[..])
        .unwrap()
        .to_string(),

      PsqlType::INT8 => postgres_protocol::types::int8_from_sql(&self.raw[..])
        .unwrap()
        .to_string(),

      PsqlType::TIMESTAMP => postgres_protocol::types::timestamp_from_sql(&self.raw[..])
        .unwrap()
        .to_string(),

      PsqlType::NUMERIC => rust_decimal::Decimal::from_sql(&self.ty, &self.raw)
        .unwrap()
        .to_string(),

      _ => String::from_utf8_lossy(&self.raw[..]).to_string(),
      // PsqlType::UUID => postgres_protocol::types::text_from_sql(&self.raw[..])
      // .unwrap()
      // .into(),

      // // ref self.ty if ty.name() == "citext" => self.enclosed_statement_value("'"),
      // _ => self.enclosed_statement_string_value(""),
    };
  }
}

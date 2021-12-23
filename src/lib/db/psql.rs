use std::collections::BTreeSet;
use std::collections::HashMap;

use diesel::pg::PgConnection;
use diesel::prelude::*;
use diesel::sql_query;
use diesel::sql_types::Text;
use itertools::Itertools;

use crate::common::types::ResultDynError;

type TableName = String;

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct PsqlTableColumn {
  pub name: String,
  pub data_type: String,
}
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct PsqlForeignKey {
  pub name: String,
  pub column: PsqlTableColumn,
  pub foreign_table_schema: String,
  pub foreign_table_name: String,
}

#[derive(PartialEq, Eq, Debug)]
pub struct PsqlTable {
  pub schema: String,
  pub name: TableName,
  pub columns: BTreeSet<PsqlTableColumn>,
  pub referenced_fk_by_constraint_name: HashMap<String, PsqlForeignKey>,
  pub referencing_fk_by_constraint_name: HashMap<String, PsqlForeignKey>,
}

#[derive(diesel::QueryableByName, PartialEq, Debug)]
struct ForeignKeyInformationRow {
  #[column_name = "table_schema"]
  #[sql_type = "Text"]
  table_schema: String,

  #[column_name = "constraint_name"]
  #[sql_type = "Text"]
  constraint_name: String,

  #[column_name = "table_name"]
  #[sql_type = "Text"]
  table_name: String,

  #[column_name = "column_name"]
  #[sql_type = "Text"]
  column_name: String,

  #[column_name = "column_data_type"]
  #[sql_type = "Text"]
  column_data_type: String,

  #[column_name = "foreign_table_schema"]
  #[sql_type = "Text"]
  foreign_table_schema: String,

  #[column_name = "foreign_table_Name"]
  #[sql_type = "Text"]
  foreign_table_name: String,

  #[column_name = "foreign_column_name"]
  #[sql_type = "Text"]
  foreign_column_name: String,
}

pub struct Psql {
  connection: PgConnection,
}

pub struct PsqlCreds {
  pub host: String,
  pub database_name: String,
  pub username: String,
  pub password: Option<String>,
}

impl Psql {
  pub fn new(creds: &PsqlCreds) -> ResultDynError<Psql> {
    let database_url = format!(
      "postgres://{}@{}/{}", // TODO: Support password
      creds.username,
      // creds.password.unwrap(),
      creds.host,
      creds.database_name
    );

    return Ok(Psql {
      connection: PgConnection::establish(&database_url)?,
    });
  }
}

impl Psql {
  pub fn load_table_structure(
    &self,
    table_name: String,
    schema: Option<String>,
  ) -> ResultDynError<PsqlTable> {
    // First try to build the UML for all of the tables
    // we'll query from psql information_schema tables.
    let fk_info_rows: Vec<ForeignKeyInformationRow> = sql_query(
      "
    SELECT
      tc.constraint_name,
      tc.table_schema,
      tc.table_name,
      kcu.column_name,
      c.data_type AS column_data_type,
      ccu.table_schema AS foreign_table_schema,
      ccu.table_name AS foreign_table_name,
      ccu.column_name AS foreign_column_name
    FROM
      information_schema.table_constraints AS tc
        JOIN information_schema.key_column_usage AS kcu
          ON tc.constraint_name = kcu.constraint_name
          AND tc.table_schema = kcu.table_schema
        JOIN information_schema.constraint_column_usage AS ccu
          ON ccu.constraint_name = tc.constraint_name
          AND ccu.table_schema = tc.table_schema
        JOIN information_schema.columns as c
          ON c.table_schema = tc.table_schema
          AND c.table_name = tc.table_name
          AND c.column_name = kcu.column_name
    WHERE tc.constraint_type = 'FOREIGN KEY';
  ",
    )
    .load(&self.connection)?;

    println!("{:?}", fk_info_rows);

    return Ok(PsqlTable {
      schema: "public".to_owned(),
      name: table_name,
      columns: Default::default(),
      referenced_fk_by_constraint_name: Default::default(),
      referencing_fk_by_constraint_name: Default::default(),
    });
  }
}

fn psql_tables_from_foreign_key_info_rows(
  rows: &Vec<ForeignKeyInformationRow>,
) -> HashMap<TableName, PsqlTable> {
  let mut table_by_name: HashMap<String, PsqlTable> = Default::default();

  let fk_info_rows_by_foreign_table_name: HashMap<String, Vec<&ForeignKeyInformationRow>> =
    rows.iter().into_group_map_by(|row| {
      return row.foreign_table_name.clone();
    });

  // Create table and fill the referencing relations,
  // we'll fill the reverse order referenced relations later on after we have
  // all of the tables data.
  for row in rows.iter() {
    table_by_name
      .entry(row.table_name.clone())
      .or_insert_with(|| {
        let mut referencing_fk_by_constraint_name: HashMap<String, PsqlForeignKey> =
          Default::default();
        let constraint_name: String = row.constraint_name.clone();

        referencing_fk_by_constraint_name.insert(
          constraint_name.clone(),
          PsqlForeignKey {
            name: constraint_name.clone(),
            column: PsqlTableColumn {
              name: row.column_name.clone(),
              data_type: row.column_data_type.clone(),
            },
            foreign_table_schema: row.foreign_table_schema.clone(),
            foreign_table_name: row.foreign_table_name.clone(),
          },
        );

        return PsqlTable {
          schema: row.table_schema.clone(),
          name: row.table_name.clone(),
          columns: Default::default(),
          referenced_fk_by_constraint_name: Default::default(),
          referencing_fk_by_constraint_name,
        };
      });
  }

  return table_by_name;
}

#[cfg(test)]
mod test {
  mod psql_tables_from_foreign_key_info_rows {
    use super::super::*;

    #[test]
    fn it_should_load_rows() {
      let fk_info_rows = vec![
        ForeignKeyInformationRow {
          table_schema: "public".into(),
          constraint_name: "ecommerce_carts_store_customer_id_foreign".into(),
          table_name: "ecommerce_carts".into(),
          column_name: "store_customer_id".into(),
          column_data_type: "integer".into(),
          foreign_table_schema: "public".into(),
          foreign_table_name: "store_customers".into(),
          foreign_column_name: "id".into(),
        },
        ForeignKeyInformationRow {
          table_schema: "public".into(),
          constraint_name: "product_variant_types_product_id_foreign".into(),
          table_name: "product_variant_types".into(),
          column_name: "product_id".into(),
          column_data_type: "integer".into(),
          foreign_table_schema: "public".into(),
          foreign_table_name: "products".into(),
          foreign_column_name: "id".into(),
        },
        ForeignKeyInformationRow {
          table_schema: "public".into(),
          constraint_name: "product_variants_product_variant_type_id_foreign".into(),
          table_name: "product_variants".into(),
          column_name: "product_variant_type_id".into(),
          column_data_type: "integer".into(),
          foreign_table_schema: "public".into(),
          foreign_table_name: "product_variant_types".into(),
          foreign_column_name: "id".into(),
        },
        ForeignKeyInformationRow {
          table_schema: "public".into(),
          constraint_name: "orders_store_id_foreign".into(),
          table_name: "orders".into(),
          column_name: "store_id".into(),
          column_data_type: "integer".into(),
          foreign_table_schema: "public".into(),
          foreign_table_name: "stores".into(),
          foreign_column_name: "id".into(),
        },
        ForeignKeyInformationRow {
          table_schema: "public".into(),
          constraint_name: "order_payment_confirmations_order_id_foreign".into(),
          table_name: "order_payment_confirmations".into(),
          column_name: "order_id".into(),
          column_data_type: "integer".into(),
          foreign_table_schema: "public".into(),
          foreign_table_name: "orders".into(),
          foreign_column_name: "id".into(),
        },
        ForeignKeyInformationRow {
          table_schema: "public".into(),
          constraint_name: "order_statuses_store_id_foreign".into(),
          table_name: "order_statuses".into(),
          column_name: "store_id".into(),
          column_data_type: "integer".into(),
          foreign_table_schema: "public".into(),
          foreign_table_name: "stores".into(),
          foreign_column_name: "id".into(),
        },
        ForeignKeyInformationRow {
          table_schema: "public".into(),
          constraint_name: "product_images_product_id_foreign".into(),
          table_name: "product_images".into(),
          column_name: "product_id".into(),
          column_data_type: "integer".into(),
          foreign_table_schema: "public".into(),
          foreign_table_name: "products".into(),
          foreign_column_name: "id".into(),
        },
        ForeignKeyInformationRow {
          table_schema: "public".into(),
          constraint_name: "product_stock_ledgers_product_id_foreign".into(),
          table_name: "product_stock_ledgers".into(),
          column_name: "product_id".into(),
          column_data_type: "integer".into(),
          foreign_table_schema: "public".into(),
          foreign_table_name: "products".into(),
          foreign_column_name: "id".into(),
        },
        ForeignKeyInformationRow {
          table_schema: "public".into(),
          constraint_name: "product_variant_templates_store_id_foreign".into(),
          table_name: "product_variant_templates".into(),
          column_name: "store_id".into(),
          column_data_type: "integer".into(),
          foreign_table_schema: "public".into(),
          foreign_table_name: "stores".into(),
          foreign_column_name: "id".into(),
        },
        ForeignKeyInformationRow {
          table_schema: "public".into(),
          constraint_name: "store_customers_store_id_foreign".into(),
          table_name: "store_customers".into(),
          column_name: "store_id".into(),
          column_data_type: "integer".into(),
          foreign_table_schema: "public".into(),
          foreign_table_name: "stores".into(),
          foreign_column_name: "id".into(),
        },
        ForeignKeyInformationRow {
          table_schema: "public".into(),
          constraint_name: "store_staffs_stores_store_staff_role_id_foreign".into(),
          table_name: "store_staffs_stores".into(),
          column_name: "store_staff_role_id".into(),
          column_data_type: "uuid".into(),
          foreign_table_schema: "public".into(),
          foreign_table_name: "store_staff_roles".into(),
          foreign_column_name: "id".into(),
        },
        ForeignKeyInformationRow {
          table_schema: "public".into(),
          constraint_name: "store_staffs_stores_store_staff_id_foreign".into(),
          table_name: "store_staffs_stores".into(),
          column_name: "store_staff_id".into(),
          column_data_type: "integer".into(),
          foreign_table_schema: "public".into(),
          foreign_table_name: "store_staffs".into(),
          foreign_column_name: "id".into(),
        },
        ForeignKeyInformationRow {
          table_schema: "public".into(),
          constraint_name: "store_staffs_stores_store_id_foreign".into(),
          table_name: "store_staffs_stores".into(),
          column_name: "store_id".into(),
          column_data_type: "integer".into(),
          foreign_table_schema: "public".into(),
          foreign_table_name: "stores".into(),
          foreign_column_name: "id".into(),
        },
        ForeignKeyInformationRow {
          table_schema: "public".into(),
          constraint_name: "tags_store_id_foreign".into(),
          table_name: "tags".into(),
          column_name: "store_id".into(),
          column_data_type: "integer".into(),
          foreign_table_schema: "public".into(),
          foreign_table_name: "stores".into(),
          foreign_column_name: "id".into(),
        },
        ForeignKeyInformationRow {
          table_schema: "public".into(),
          constraint_name: "products_store_id_foreign".into(),
          table_name: "products".into(),
          column_name: "store_id".into(),
          column_data_type: "integer".into(),
          foreign_table_schema: "public".into(),
          foreign_table_name: "stores".into(),
          foreign_column_name: "id".into(),
        },
        ForeignKeyInformationRow {
          table_schema: "public".into(),
          constraint_name: "order_items_order_id_foreign".into(),
          table_name: "order_items".into(),
          column_name: "order_id".into(),
          column_data_type: "integer".into(),
          foreign_table_schema: "public".into(),
          foreign_table_name: "orders".into(),
          foreign_column_name: "id".into(),
        },
        ForeignKeyInformationRow {
          table_schema: "public".into(),
          constraint_name: "tagged_models_tag_id_foreign".into(),
          table_name: "tagged_models".into(),
          column_name: "tag_id".into(),
          column_data_type: "integer".into(),
          foreign_table_schema: "public".into(),
          foreign_table_name: "tags".into(),
          foreign_column_name: "id".into(),
        },
      ];

      let psql_tables: HashMap<TableName, PsqlTable> =
        psql_tables_from_foreign_key_info_rows(&fk_info_rows);

      let order_items_table: &PsqlTable = psql_tables.get("order_items").unwrap();

      // Make sure relations are set correctly
      assert_eq!(order_items_table.name, "order_items");
      assert_eq!(order_items_table.referencing_fk_by_constraint_name.len(), 1);

      let fk_to_orders_table_from_order_items = order_items_table
        .referencing_fk_by_constraint_name
        .get("order_items_order_id_foreign");

      assert!(fk_to_orders_table_from_order_items.is_some());

      // Make sure created tables have equal size
      // with unique table names in fk info rows
      let available_tables: BTreeSet<&String> =
        fk_info_rows.iter().map(|row| &row.table_name).collect();

      assert_eq!(psql_tables.len(), available_tables.len())
    }
  }
}

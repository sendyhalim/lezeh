use std::any::Any;
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

pub struct DbFetcher {
  pub psql_table_by_name: HashMap<TableName, PsqlTable>,
  psql: Psql,
}

pub struct FetchRowInput {
  pub table_name: String,
  pub id: String,
}

impl DbFetcher {
  fn fetch_row(&self, input: &FetchRowInput) -> ResultDynError<Vec<Vec<Box<dyn Any>>>> {
    let psql_table = self.psql_table_by_name.get(&input.table_name);

    if psql_table.is_none() {
      return Ok(vec![]);
    }

    let rows: Vec<HashMap<String, String>> = sql_query(format!(
      "
    SELECT * FROM {} where id = {}
  ",
      input.table_name, input.id
    ))
    .get_result(&self.psql.connection)?;

    // Try to fetch the row first
    // If it exists
    //   create table relation b tree where the key is table name
    //   check whether it has referencing tables (depends on its parent tables)
    //     if yes then
    //       parent_tables = map referencing tables as parent_table
    //         parent = fetch go up 1 level by fetch_referencing_rows(
    //           criteria: {
    //             id: currentRow[referencing_column]
    //             table: referencing_table
    //           },
    //           current_iteration: parent_table
    //         )
    //     otherwise
    //       register the current table as root table
    //       fetch the current row by
    //          select * from {input.table_name} where id = {input.id}
    //   check whether it has referenced tables (has children tables)
    //     if yes then
    //       child_tables = map referenced tables as child_tables
    //       children = fetch 1 level down by fetch_referenced_rows(
    //           criteria: {
    //             id: currentRow[referenced_column]
    //             table: referenced_table
    //           },
    //           current_iteration: child_table
    //       )
    //     otherwise stop
    // else
    //   return
    return vec![];
  }

  // fn fetch_referencing_rows(table_tree: ) {
  // }

  // fn fetch_referenced_rows(table_tree: ) {
  // }
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
    let table = table_by_name
      .entry(row.table_name.clone())
      .or_insert(PsqlTable {
        schema: row.table_schema.clone(),
        name: row.table_name.clone(),
        columns: Default::default(),
        referenced_fk_by_constraint_name: Default::default(),
        referencing_fk_by_constraint_name: Default::default(),
      });

    // We can start filling referencing_fk data first
    // because every row contains info of how a table references another table
    let constraint_name: String = row.constraint_name.clone();

    table.referencing_fk_by_constraint_name.insert(
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

    table.referenced_fk_by_constraint_name =
      fk_info_rows_by_foreign_table_name.get(&table.name).map_or(
        Default::default(),
        |fk_info_rows: &Vec<&ForeignKeyInformationRow>| -> HashMap<String, PsqlForeignKey> {
          return fk_info_rows
            .iter()
            .map(|row| {
              return (
                row.constraint_name.clone(),
                PsqlForeignKey {
                  name: row.constraint_name.clone(),
                  column: PsqlTableColumn {
                    name: row.column_name.clone(),
                    data_type: row.column_data_type.clone(),
                  },
                  foreign_table_schema: row.foreign_table_schema.clone(),
                  foreign_table_name: row.foreign_table_name.clone(),
                },
              );
            })
            .collect();
        },
      );
  }

  return table_by_name;
}

#[cfg(test)]
mod test {
  mod psql_tables_from_foreign_key_info_rows {
    use super::super::*;

    #[test]
    fn it_should_load_rows() {
      // Db diagram view https://dbdiagram.io/d/6205540d85022f4ee57331e2
      let fk_info_rows = vec![
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
          constraint_name: "order_items_product_id_foreign".into(),
          table_name: "order_items".into(),
          column_name: "product_id".into(),
          column_data_type: "integer".into(),
          foreign_table_schema: "public".into(),
          foreign_table_name: "products".into(),
          foreign_column_name: "id".into(),
        },
      ];

      let psql_tables: HashMap<TableName, PsqlTable> =
        psql_tables_from_foreign_key_info_rows(&fk_info_rows);

      // Make sure relations are set correctly
      // -------------------------------------------
      // table: order_items
      let order_items_table: &PsqlTable = psql_tables.get("order_items").unwrap();

      assert_eq!(order_items_table.name, "order_items");
      assert_eq!(order_items_table.referencing_fk_by_constraint_name.len(), 2);
      assert_eq!(order_items_table.referenced_fk_by_constraint_name.len(), 0);

      let fk_to_orders_table_from_order_items = order_items_table
        .referencing_fk_by_constraint_name
        .get("order_items_order_id_foreign");

      assert!(fk_to_orders_table_from_order_items.is_some());

      // table: store_staffs_stores
      let store_staffs_stores_table: &PsqlTable = psql_tables.get("store_staffs_stores").unwrap();
      assert_eq!(store_staffs_stores_table.name, "store_staffs_stores");
      assert_eq!(
        store_staffs_stores_table
          .referencing_fk_by_constraint_name
          .len(),
        3
      );

      // table: store_staffs_stores
      let products_table: &PsqlTable = psql_tables.get("products").unwrap();
      assert_eq!(products_table.name, "products");
      assert_eq!(products_table.referencing_fk_by_constraint_name.len(), 1);
      assert_eq!(products_table.referenced_fk_by_constraint_name.len(), 3);

      // Make sure created tables have equal size
      // with unique table names in fk info rows
      // -------------------------------------------
      let available_tables: BTreeSet<&String> =
        fk_info_rows.iter().map(|row| &row.table_name).collect();

      assert_eq!(psql_tables.len(), available_tables.len())
    }
  }
}

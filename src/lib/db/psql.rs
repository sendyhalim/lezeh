use std::any::Any;
use std::cell::RefCell;
use std::collections::BTreeSet;
use std::collections::HashMap;

use anyhow::anyhow;
use itertools::Itertools;
use postgres::config::Config as PsqlConfig;
use postgres::Client as PsqlClient;
use postgres::Row;

use crate::common::types::ResultAnyError;

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

pub struct RoseTreeNode<T> {
  pub parent: RefCell<Vec<RoseTreeNode<T>>>,
  pub children: RefCell<Vec<RoseTreeNode<T>>>,
  pub value: T,
}

impl<T> RoseTreeNode<T> {
  fn new(value: T) -> RoseTreeNode<T> {
    return RoseTreeNode {
      parent: Default::default(),
      children: Default::default(),
      value,
    };
  }
}

#[derive(PartialEq, Debug)]
pub struct ForeignKeyInformationRow {
  table_schema: String,

  constraint_name: String,

  table_name: String,

  column_name: String,

  column_data_type: String,

  foreign_table_schema: String,

  foreign_table_name: String,

  foreign_column_name: String,
}

pub struct Psql {
  client: PsqlClient,
}

pub struct PsqlCreds {
  pub host: String,
  pub database_name: String,
  pub username: String,
  pub password: Option<String>,
}

impl Psql {
  pub fn new(creds: &PsqlCreds) -> ResultAnyError<Psql> {
    return Ok(Psql {
      client: PsqlConfig::new()
        .user(&creds.username)
        // .password(creds.password.as_ref().unwrap()) // Should defaults to empty binary
        .host(&creds.host)
        .dbname(&creds.database_name)
        .connect(postgres::NoTls)?,
    });
  }
}

impl Psql {
  pub fn load_table_structure(
    &mut self,
    schema: Option<String>,
  ) -> ResultAnyError<Vec<ForeignKeyInformationRow>> {
    // let a = RoseTreeNode::new("foo");

    // a.parent.get_mut().push(RoseTreeNode::new("hi"));

    // First try to build the UML for all of the tables
    // we'll query from psql information_schema tables.
    let rows: Vec<Row> = self.client.query(
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
      &[],
    )?;

    let fk_info_rows: Vec<ForeignKeyInformationRow> = rows
      .into_iter()
      .map(|row: Row| -> ForeignKeyInformationRow {
        return ForeignKeyInformationRow {
          table_schema: row.get("table_schema"),
          constraint_name: row.get("constraint_name"),
          table_name: row.get("table_name"),
          column_name: row.get("column_name"),
          column_data_type: row.get("column_data_type"),
          foreign_table_schema: row.get("foreign_table_schema"),
          foreign_table_name: row.get("foreign_table_name"),
          foreign_column_name: row.get("foreign_column_name"),
        };
      })
      .collect();

    // println!("{:#?}", fk_info_rows);

    return Ok(fk_info_rows);
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
  fn fetch_row(&mut self, input: &FetchRowInput) -> ResultAnyError<Vec<Vec<Box<dyn Any>>>> {
    unimplemented!("unimplemented");
    let psql_table = self.psql_table_by_name.get(&input.table_name);

    if psql_table.is_none() {
      return Ok(vec![]);
    }

    let rows: Vec<Row> = self.psql.client.query(
      "
    SELECT * FROM $1 where id = $2
    ",
      &[&input.table_name, &input.id],
    )?;

    let row: &Row = rows
      .get(0)
      .ok_or_else(|| anyhow!("Could not find row with id {}", input.id))?;

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

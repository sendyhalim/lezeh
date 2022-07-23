use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use itertools::Itertools;
use postgres::types::ToSql;
use postgres::Row;

use crate::common::types::ResultAnyError;
use crate::db::psql::connection::PsqlConnection;
use crate::db::psql::dto::*;

pub type PsqlParamValue = Box<dyn ToSql + Sync>;

const TABLE_WITH_FK_QUERY: &'static str = "
    SELECT
      tc.constraint_name,
      tc.table_schema,
      tc.table_name,
      kcu.column_name,
      c.data_type AS column_data_type,
      ccu.table_schema AS foreign_table_schema,
      ccu.table_name AS foreign_table_name,
      ccu.column_name AS foreign_column_name,
      foreign_c_meta.data_type AS foreign_column_data_type
    FROM
      information_schema.table_constraints AS tc
        JOIN information_schema.key_column_usage AS kcu ON
          tc.constraint_name = kcu.constraint_name AND
          tc.table_schema = kcu.table_schema
        JOIN information_schema.constraint_column_usage AS ccu ON
          ccu.constraint_name = tc.constraint_name
        JOIN information_schema.columns as c ON
          c.table_name = tc.table_name AND
          c.column_name = kcu.column_name
        JOIN information_schema.columns as foreign_c_meta ON
          foreign_c_meta.table_schema = ccu.table_schema AND
          foreign_c_meta.table_name = ccu.table_name AND
          foreign_c_meta.column_name = ccu.column_name
    WHERE tc.constraint_type = 'FOREIGN KEY';
";

#[derive(PartialEq, Debug)]
pub struct ForeignKeyInformationRow {
  constraint_name: String,

  // From table X
  table_schema: String,
  table_name: String,
  column_name: String,
  column_data_type: String,

  // referencing to table Y
  foreign_table_schema: String,
  foreign_table_name: String,
  foreign_column_name: String,
  foreign_column_data_type: String,
}

pub struct Query {
  connection: Rc<RefCell<PsqlConnection>>,
}

impl Query {
  fn fetch_fk_info(&mut self, _schema: &str) -> ResultAnyError<Vec<ForeignKeyInformationRow>> {
    // First try to build the UML for all of the tables
    // we'll query from psql information_schema tables.
    let rows: Vec<Row> = self
      .connection
      .borrow_mut()
      .get()
      .query(TABLE_WITH_FK_QUERY, &[])?;

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
          foreign_column_data_type: row.get("foreign_column_data_type"),
        };
      })
      .collect();

    return Ok(fk_info_rows);
  }

  fn get_table_by_id(&mut self) -> ResultAnyError<HashMap<PsqlTableIdentity, PsqlTable>> {
    let rows: Vec<Row> = self.connection.borrow_mut().get().query(
      "
      SELECT
        tc.constraint_name,
        tc.table_schema,
        tc.table_name,
        kcu.column_name as primary_column_name,
        c.data_type AS primary_column_data_type
      FROM
        information_schema.table_constraints AS tc
          JOIN information_schema.key_column_usage AS kcu
            ON tc.constraint_name = kcu.constraint_name
            AND tc.table_schema = kcu.table_schema
          JOIN information_schema.columns as c
            ON c.table_schema = tc.table_schema
            AND c.table_name = tc.table_name
            AND c.column_name = kcu.column_name
      WHERE tc.constraint_type = 'PRIMARY KEY' and
       tc.table_schema not in ('pg_catalog', 'information_schema')
      ",
      &[],
    )?;

    let psql_table_by_id: HashMap<PsqlTableIdentity, PsqlTable> = rows
      .into_iter()
      .map(|row| {
        let psql_table = PsqlTable::new(
          row.get::<_, String>("table_schema"),
          row.get::<_, String>("table_name"),
          PsqlTableColumn::new(
            row.get::<_, String>("primary_column_name"),
            row.get::<_, String>("primary_column_data_type"),
          ),
          Default::default(),
          Default::default(),
          Default::default(),
        );

        return (psql_table.id.clone(), psql_table);
      })
      .collect();

    return Ok(psql_table_by_id);
  }
}

pub struct DbMetadata {
  /// We know that we own this query so it's ok
  /// to directl borrow_mut() without checking ownership
  query: RefCell<Query>,
}

impl DbMetadata {
  pub fn new(psql_connection: Rc<RefCell<PsqlConnection>>) -> DbMetadata {
    return DbMetadata {
      query: RefCell::new(Query {
        connection: psql_connection,
      }),
    };
  }
}

impl DbMetadata {
  pub fn load_table_structure(
    &self,
    schema: &str,
  ) -> ResultAnyError<HashMap<PsqlTableIdentity, PsqlTable>> {
    let fk_info_rows = self.query.borrow_mut().fetch_fk_info(schema)?;

    let mut table_by_id = self.query.borrow_mut().get_table_by_id()?;

    psql_table_map_from_foreign_key_info_rows(&mut table_by_id, &fk_info_rows);

    return Ok(table_by_id);
  }
}

fn psql_table_map_from_foreign_key_info_rows(
  table_by_id: &mut HashMap<PsqlTableIdentity, PsqlTable>,
  rows: &Vec<ForeignKeyInformationRow>,
) {
  let fk_info_rows_by_foreign_table_id: HashMap<PsqlTableIdentity, Vec<&ForeignKeyInformationRow>> =
    rows.iter().into_group_map_by(|row| {
      return PsqlTableIdentity::new(&row.foreign_table_schema, &row.foreign_table_name);
    });

  let fk_info_rows_by_table_id: HashMap<PsqlTableIdentity, Vec<&ForeignKeyInformationRow>> =
    rows.iter().into_group_map_by(|row| {
      return PsqlTableIdentity::new(&row.table_schema, &row.table_name);
    });

  for (table_id, table) in table_by_id.into_iter() {
    let referencing_fk_rows = fk_info_rows_by_table_id.get(&table_id);

    if referencing_fk_rows.is_some() {
      let referencing_fk_rows = referencing_fk_rows.unwrap();

      table.referencing_fk_by_constraint_name = referencing_fk_rows
        .iter()
        .map(|fk_row| {
          return (
            fk_row.constraint_name.clone(),
            PsqlForeignKey::new(
              fk_row.constraint_name.clone(),
              PsqlTableColumn::new(fk_row.column_name.clone(), fk_row.column_data_type.clone()),
              fk_row.foreign_table_schema.clone(),
              fk_row.foreign_table_name.clone(),
            ),
          );
        })
        .collect();
    }

    let referenced_fk_rows = fk_info_rows_by_foreign_table_id.get(&table_id);

    if referenced_fk_rows.is_some() {
      let referenced_fk_rows = referenced_fk_rows.unwrap();

      table.referenced_fk_by_constraint_name = referenced_fk_rows
        .iter()
        .map(|fk_row| {
          return (
            fk_row.constraint_name.clone(),
            PsqlForeignKey::new(
              fk_row.constraint_name.clone(),
              PsqlTableColumn::new(fk_row.column_name.clone(), fk_row.column_data_type.clone()),
              fk_row.table_schema.clone(),
              fk_row.table_name.clone(),
            ),
          );
        })
        .collect();
    }
  }
}

#[cfg(test)]
mod test {
  use super::*;
  use std::borrow::Cow;
  use std::collections::HashSet;

  impl PsqlTable {
    fn basic<'a, S>(schema: S, name: S, primary_column: PsqlTableColumn) -> PsqlTable
    where
      S: Into<Cow<'a, str>>,
    {
      return PsqlTable {
        id: PsqlTableIdentity::new(schema, name),
        primary_column,
        columns: Default::default(),
        referenced_fk_by_constraint_name: Default::default(),
        referencing_fk_by_constraint_name: Default::default(),
      };
    }
  }

  mod psql_tables_from_foreign_key_info_rows {
    use super::*;
    use crate::common::macros::hashmap_literal;

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
          foreign_column_data_type: "integer".into(),
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
          foreign_column_data_type: "integer".into(),
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
          foreign_column_data_type: "integer".into(),
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
          foreign_column_data_type: "integer".into(),
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
          foreign_column_data_type: "integer".into(),
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
          foreign_column_data_type: "uuid".into(),
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
          foreign_column_data_type: "integer".into(),
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
          foreign_column_data_type: "integer".into(),
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
          foreign_column_data_type: "integer".into(),
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
          foreign_column_data_type: "integer".into(),
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
          foreign_column_data_type: "integer".into(),
        },
      ];

      let mut psql_table_by_id: HashMap<PsqlTableIdentity, PsqlTable> = hashmap_literal! {
        PsqlTableIdentity::new("public", "stores") => PsqlTable::basic("public", "stores", PsqlTableColumn{
          name: "id".into(),
          data_type: "integer".into(),
        }),
        PsqlTableIdentity::new("public", "orders") => PsqlTable::basic("public", "orders", PsqlTableColumn{
          name: "id".into(),
          data_type: "integer".into(),
        }),
        PsqlTableIdentity::new("public", "order_items") => PsqlTable::basic("public", "order_items", PsqlTableColumn{
          name: "id".into(),
          data_type: "integer".into(),
        }),
        PsqlTableIdentity::new("public", "order_statuses") => PsqlTable::basic("public", "order_statuses", PsqlTableColumn{
          name: "id".into(),
          data_type: "integer".into(),
        }),
        PsqlTableIdentity::new("public", "products") => PsqlTable::basic("public", "products", PsqlTableColumn{
          name: "id".into(),
          data_type: "integer".into(),
        }),
        PsqlTableIdentity::new("public", "product_images") => PsqlTable::basic("public", "product_images", PsqlTableColumn{
          name: "id".into(),
          data_type: "integer".into(),
        }),
        PsqlTableIdentity::new("public", "product_stock_ledgers") => PsqlTable::basic("public", "product_stock_ledgers", PsqlTableColumn{
          name: "id".into(),
          data_type: "integer".into(),
        }),
        PsqlTableIdentity::new("public", "store_customers") => PsqlTable::basic("public", "store_customers", PsqlTableColumn{
          name: "id".into(),
          data_type: "integer".into(),
        }),
        PsqlTableIdentity::new("public", "store_staffs_stores") => PsqlTable::basic("public", "store_staffs_stores", PsqlTableColumn{
          name: "id".into(),
          data_type: "uuid".into(),
        }),
        PsqlTableIdentity::new("public", "store_staff_roles") => PsqlTable::basic("public", "store_staff_roles", PsqlTableColumn{
          name: "id".into(),
          data_type: "uuid".into(),
        }),
        PsqlTableIdentity::new("public", "store_staffs") => PsqlTable::basic("public", "store_staffs", PsqlTableColumn{
          name: "id".into(),
          data_type: "integer".into(),
        }),
      };

      // TODO: Need to prefil psql tables
      psql_table_map_from_foreign_key_info_rows(&mut psql_table_by_id, &fk_info_rows);

      // Make sure relations are set correctly
      // -------------------------------------------
      // table: order_items
      let order_items_table: &PsqlTable = psql_table_by_id
        .get(&PsqlTableIdentity::new("public", "order_items"))
        .unwrap();

      assert_eq!(
        order_items_table.id,
        PsqlTableIdentity::new("public", "order_items")
      );
      assert_eq!(order_items_table.referencing_fk_by_constraint_name.len(), 2);
      assert_eq!(order_items_table.referenced_fk_by_constraint_name.len(), 0);

      let fk_to_orders_table_from_order_items = order_items_table
        .referencing_fk_by_constraint_name
        .get("order_items_order_id_foreign");

      assert!(fk_to_orders_table_from_order_items.is_some());

      // table: store_staffs_stores
      let store_staffs_stores_table: &PsqlTable = psql_table_by_id
        .get(&PsqlTableIdentity::new("public", "store_staffs_stores"))
        .ok_or_else(|| "could not get store_staffs_stores")
        .unwrap();

      assert_eq!(
        store_staffs_stores_table.id,
        PsqlTableIdentity::new("public", "store_staffs_stores")
      );
      assert_eq!(
        store_staffs_stores_table
          .referencing_fk_by_constraint_name
          .len(),
        3
      );

      // table: store_staffs_stores
      let products_table: &PsqlTable = psql_table_by_id
        .get(&PsqlTableIdentity::new("public", "products"))
        .unwrap();

      assert_eq!(
        products_table.id,
        PsqlTableIdentity::new("public", "products")
      );

      assert_eq!(products_table.referencing_fk_by_constraint_name.len(), 1);
      assert_eq!(products_table.referenced_fk_by_constraint_name.len(), 3);

      // Make sure created tables have equal size
      // with unique table names in fk info rows
      // -------------------------------------------
      let _available_tables: HashSet<&String> =
        fk_info_rows.iter().map(|row| &row.table_name).collect();

      assert_eq!(psql_table_by_id.len(), 11)
    }
  }
}

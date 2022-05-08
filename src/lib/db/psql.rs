use std::cell::RefCell;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::rc::Rc;

use anyhow::anyhow;
use itertools::Itertools;
use postgres::config::Config as PsqlConfig;
use postgres::Client as PsqlClient;
use postgres::Row;

use crate::common::types::ResultAnyError;
type TableName = String;

#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone)]
pub struct PsqlTableColumn {
  pub name: String,
  pub data_type: String,
}
#[derive(PartialEq, Eq, PartialOrd, Ord, Debug, Clone)]
pub struct PsqlForeignKey {
  pub name: String,
  pub column: PsqlTableColumn,
  pub foreign_table_schema: String,
  pub foreign_table_name: String,
}

#[derive(PartialEq, Eq, Debug, Clone)]
pub struct PsqlTable {
  pub schema: String,
  pub name: TableName,
  pub primary_column: PsqlTableColumn,
  pub columns: BTreeSet<PsqlTableColumn>,
  pub referenced_fk_by_constraint_name: HashMap<String, PsqlForeignKey>,
  pub referencing_fk_by_constraint_name: HashMap<String, PsqlForeignKey>,
}

#[derive(Debug, Clone)]
pub struct PsqlTableRows {
  pub table: PsqlTable,
  pub rows: Vec<Rc<Row>>,
}

/// RoseTreeNode can start from multiple roots
#[derive(Debug, Clone)]
pub struct RoseTreeNode<T> {
  pub parents: RefCell<Vec<RoseTreeNode<T>>>,
  pub children: RefCell<Vec<RoseTreeNode<T>>>,
  pub value: T,
}

impl<T> RoseTreeNode<T> {
  fn new(value: T) -> RoseTreeNode<T> {
    return RoseTreeNode {
      parents: Default::default(),
      children: Default::default(),
      value,
    };
  }
}

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
        JOIN information_schema.columns as foreign_c_meta
          ON foreign_c_meta.table_schema = ccu.table_schema
          AND foreign_c_meta.table_name = ccu.table_name
          AND foreign_c_meta.column_name = ccu.column_name
    WHERE tc.constraint_type = 'FOREIGN KEY';
";

impl Psql {
  pub fn fetch_fk_info(
    &mut self,
    _schema: Option<String>,
  ) -> ResultAnyError<Vec<ForeignKeyInformationRow>> {
    // First try to build the UML for all of the tables
    // we'll query from psql information_schema tables.
    let rows: Vec<Row> = self.client.query(TABLE_WITH_FK_QUERY, &[])?;

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

  // fn fetch_tables_without_fk(&mut self, _schema: Option<String>) -> ResultAnyError<HashMap> {
  //   let table_without_fk_query: String = format!(
  //     "
  //       with table_with_fk as (
  //           {}
  //       )
  //       select table_schema, table_name from information_schema.tables as t
  //         where
  //           t.table_name not in (select table_name from table_with_fk) and
  //           t.table_schema not in('pg_catalog', 'information_schema');
  //   ",
  //     TABLE_WITH_FK_QUERY
  //   );

  //   return self
  //     .client
  //     .query(&table_without_fk_query, &[])
  //     .map_err(anyhow::Error::from);
  // }

  pub fn load_table_structure(
    &mut self,
    schema: Option<String>,
  ) -> ResultAnyError<HashMap<String, PsqlTable>> {
    let fk_info_rows = self.fetch_fk_info(schema)?;

    let mut table_by_name: HashMap<String, PsqlTable> = self.get_table_by_name()?;

    psql_table_map_from_foreign_key_info_rows(&mut table_by_name, &fk_info_rows);

    return Ok(table_by_name);
  }

  pub fn get_table_by_name(&mut self) -> ResultAnyError<HashMap<String, PsqlTable>> {
    let rows: Vec<Row> = self.client.query(
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

    let psql_table_by_name: HashMap<String, PsqlTable> = rows
      .into_iter()
      .map(|row| {
        let psql_table = PsqlTable {
          schema: row.get("table_schema"),
          name: row.get("table_name"),
          columns: Default::default(),
          primary_column: PsqlTableColumn {
            name: row.get("primary_column_name"),
            data_type: row.get("primary_column_data_type"),
          },
          referenced_fk_by_constraint_name: Default::default(),
          referencing_fk_by_constraint_name: Default::default(),
        };

        return (psql_table.name.clone(), psql_table);
      })
      .collect();

    return Ok(psql_table_by_name);
  }
}

pub struct DbFetcher {
  pub psql_table_by_name: HashMap<TableName, PsqlTable>,
  pub psql: Psql,
}

pub struct FetchRowInput {
  pub schema: Option<String>,
  pub table_name: String,
  pub column_name: String,
  pub column_value: String,
}

impl DbFetcher {
  fn find_one_row(&mut self, input: &FetchRowInput) -> ResultAnyError<Option<Row>> {
    let rows_result = self.find_rows(input);

    return match rows_result {
      Err(any) => Err(any),
      Ok(mut rows) => {
        if rows.len() > 1 {
          return Err(anyhow!("Too many rows returned {:?}", rows));
        }

        if rows.len() == 0 {
          return Ok(None);
        }

        return Ok(Some(rows.remove(0)));
      }
    };
  }

  fn find_rows(&mut self, input: &FetchRowInput) -> ResultAnyError<Vec<Row>> {
    let query_str = format!(
      "SELECT * FROM {} where {} = {}",
      input.table_name, input.column_name, input.column_value
    );

    println!("Fetching row with query `{}`", query_str);

    return self
      .psql
      .client
      .query(&query_str, &[])
      .map_err(anyhow::Error::from);
  }

  fn get_id_from_row(row: &Row, id_column_spec: &PsqlTableColumn) -> String {
    if id_column_spec.data_type == "integer" {
      return format!("{}", row.get::<'_, _, i32>(id_column_spec.name.as_str()));
    }

    return row.get::<'_, _, String>(id_column_spec.name.as_str());
  }

  pub fn fetch_rose_trees_to_be_inserted(
    &mut self,
    input: &FetchRowInput,
  ) -> ResultAnyError<Vec<RoseTreeNode<PsqlTableRows>>> {
    let psql_table = self.psql_table_by_name.get(&input.table_name);

    println!("{:#?}", self.psql_table_by_name);
    if psql_table.is_none() {
      return Ok(vec![]);
    }

    let row: Row = self.find_one_row(input)?.ok_or_else(|| {
      anyhow!(
        "Could not find row {} {} in table {}",
        input.column_name,
        input.column_value,
        input.table_name
      )
    })?;

    let row: Rc<Row> = Rc::new(row);

    // We just fetched it, let's just assume naively that the table
    // will still exist right after we fetch it.
    let current_table: PsqlTable = self
      .psql_table_by_name
      .get(&input.table_name)
      .unwrap()
      .clone();

    // Fill the relationships in upper layers (parents)
    // ----------------------------------------
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
    let mut row_node: RoseTreeNode<PsqlTableRows> = RoseTreeNode::new(PsqlTableRows {
      table: current_table.clone(),
      rows: vec![row.clone()],
    });

    if !current_table.referencing_fk_by_constraint_name.is_empty() {
      let parents: Vec<RoseTreeNode<PsqlTableRows>> = current_table
        .referencing_fk_by_constraint_name
        .iter()
        .map(|(_key, psql_foreign_key)| {
          return self
            .fetch_referencing_row(
              psql_foreign_key.clone(),
              DbFetcher::get_id_from_row(row.as_ref(), &psql_foreign_key.column),
            )
            .unwrap();
        })
        .collect();

      row_node.parents = RefCell::new(parents);
    }

    // Fill the relationships in lower layers (parents)
    // ----------------------------------------
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

    if !current_table.referenced_fk_by_constraint_name.is_empty() {
      let children: Vec<RoseTreeNode<PsqlTableRows>> = current_table
        .referenced_fk_by_constraint_name
        .iter()
        .filter_map(|(_key, psql_foreign_key)| {
          // println!(
          //   "[OUTER] fetching referenced rows {:?} {:?} {:?}",
          //   psql_foreign_key, row, &current_table.primary_column
          // );

          return self
            .fetch_referenced_rows(
              psql_foreign_key.clone(),
              // DbFetcher::get_id_from_row(row.as_ref(), &current_table.primary_column),
              DbFetcher::get_id_from_row(row.as_ref(), &current_table.primary_column),
            )
            .unwrap();
        })
        .collect();

      row_node.children = RefCell::new(children);
    }

    return Ok(vec![row_node]);
  }

  fn fetch_referencing_row(
    &mut self,
    foreign_key: PsqlForeignKey,
    id: String,
  ) -> ResultAnyError<RoseTreeNode<PsqlTableRows>> {
    let table: PsqlTable = self
      .psql_table_by_name
      .get(&foreign_key.foreign_table_name)
      .unwrap()
      .clone();

    let mut row_node = self.create_initial_node_from_row(
      foreign_key.foreign_table_schema,
      foreign_key.foreign_table_name,
      table.primary_column.name,
      id,
    )?;

    // We  know we'll always have that 1 row
    let row = row_node.value.rows.get(0).unwrap();

    // This method should be called from lower level, so we just need to go to upper level
    let parents: Vec<RoseTreeNode<PsqlTableRows>> = table
      .referencing_fk_by_constraint_name
      .iter()
      .map(|(_key, psql_foreign_key)| {
        return self
          .fetch_referencing_row(
            psql_foreign_key.clone(),
            DbFetcher::get_id_from_row(row.as_ref(), &psql_foreign_key.column),
          )
          .unwrap();
      })
      .collect();

    row_node.parents = RefCell::new(parents);

    return Ok(row_node);
  }

  fn fetch_referenced_rows(
    &mut self,
    foreign_key: PsqlForeignKey,
    id: String,
  ) -> ResultAnyError<Option<RoseTreeNode<PsqlTableRows>>> {
    // println!("fetching referenced rows {:?} {:?}", foreign_key, id);
    let table: PsqlTable = self
      .psql_table_by_name
      .get(&foreign_key.foreign_table_name)
      .unwrap()
      .clone();

    println!("Creating initial node from table row {} {}", table.name, id);

    let mut row_node = self.create_initial_node_from_row(
      table.schema,
      table.name.clone(),
      foreign_key.column.name,
      id,
    )?;

    println!(
      "Referenced rows contains {} rows",
      row_node.value.rows.len(),
    );
    if row_node.value.rows.is_empty() {
      return Ok(None);
    }

    // We  know we'll always have that 1 row
    let row = row_node.value.rows.get(0).unwrap();

    println!(
      "{} Continue to fetch referenced rows {:?}",
      table.name.clone(),
      table.referenced_fk_by_constraint_name
    );
    // This method should be called from lower level, so we just need to go to upper level
    let children: Vec<RoseTreeNode<PsqlTableRows>> = table
      .referenced_fk_by_constraint_name
      .iter()
      .filter_map(|(_key, psql_foreign_key)| {
        return self
          .fetch_referenced_rows(
            psql_foreign_key.clone(),
            DbFetcher::get_id_from_row(row.as_ref(), &psql_foreign_key.column),
          )
          .unwrap();
      })
      .collect();

    row_node.children = RefCell::new(children);

    return Ok(Some(row_node));
  }

  fn create_initial_node_from_row(
    &mut self,
    schema: String,
    table_name: String,
    id_column_name: String,
    id: String,
  ) -> ResultAnyError<RoseTreeNode<PsqlTableRows>> {
    let table: PsqlTable = self
      .psql_table_by_name
      .get(&table_name)
      .ok_or_else(|| anyhow!("Could not find table {}", table_name))?
      .clone();

    let rows = self.find_rows(&FetchRowInput {
      schema: Some(schema),
      table_name,
      column_name: id_column_name.clone(),
      column_value: id,
    })?;

    let node = RoseTreeNode::new(PsqlTableRows {
      table,
      rows: rows.into_iter().map(Rc::new).collect(),
    });

    return Ok(node);
  }
}

fn psql_table_map_from_foreign_key_info_rows(
  table_by_name: &mut HashMap<String, PsqlTable>,
  rows: &Vec<ForeignKeyInformationRow>,
) {
  let fk_info_rows_by_foreign_table_name: HashMap<String, Vec<&ForeignKeyInformationRow>> =
    rows.iter().into_group_map_by(|row| {
      return row.foreign_table_name.clone();
    });

  let fk_info_rows_by_table_name: HashMap<String, Vec<&ForeignKeyInformationRow>> =
    rows.iter().into_group_map_by(|row| {
      return row.table_name.clone();
    });

  for (table_name, table) in table_by_name.into_iter() {
    let referencing_fk_rows = fk_info_rows_by_table_name.get(table_name);

    if referencing_fk_rows.is_some() {
      let referencing_fk_rows = referencing_fk_rows.unwrap();

      table.referencing_fk_by_constraint_name = referencing_fk_rows
        .iter()
        .map(|fk_row| {
          return (
            fk_row.constraint_name.clone(),
            PsqlForeignKey {
              name: fk_row.constraint_name.clone(),
              column: PsqlTableColumn {
                name: fk_row.column_name.clone(),
                data_type: fk_row.column_data_type.clone(),
              },
              foreign_table_schema: fk_row.foreign_table_schema.clone(),
              foreign_table_name: fk_row.foreign_table_name.clone(),
            },
          );
        })
        .collect();
    }

    let referenced_fk_rows = fk_info_rows_by_foreign_table_name.get(table_name);

    if referenced_fk_rows.is_some() {
      let referenced_fk_rows = referenced_fk_rows.unwrap();

      table.referenced_fk_by_constraint_name = referenced_fk_rows
        .iter()
        .map(|fk_row| {
          return (
            fk_row.constraint_name.clone(),
            PsqlForeignKey {
              name: fk_row.constraint_name.clone(),
              column: PsqlTableColumn {
                name: fk_row.column_name.clone(),
                data_type: fk_row.column_data_type.clone(),
              },
              foreign_table_schema: fk_row.table_schema.clone(),
              foreign_table_name: fk_row.table_name.clone(),
            },
          );
        })
        .collect();
    }
  }

  // Create table and fill the referencing relations,
  // we'll fill the reverse order referenced relations later on after we have
  // all of the tables data.
  // for row in rows.iter() {
  //   let table = table_by_name.get_mut(&row.table_name);

  //   if table.is_none() {
  //     continue;
  //   }

  //   let mut table = table.unwrap();

  //   // We can start filling referencing_fk data first
  //   // because every row contains info of how a table references another table
  //   let constraint_name: String = row.constraint_name.clone();

  //   table.referencing_fk_by_constraint_name.insert(
  //     constraint_name.clone(),
  //     PsqlForeignKey {
  //       name: constraint_name.clone(),
  //       column: PsqlTableColumn {
  //         name: row.column_name.clone(),
  //         data_type: row.column_data_type.clone(),
  //       },
  //       foreign_table_schema: row.foreign_table_schema.clone(),
  //       foreign_table_name: row.foreign_table_name.clone(),
  //     },
  //   );

  //   table.referenced_fk_by_constraint_name =
  //     fk_info_rows_by_foreign_table_name.get(&table.name).map_or(
  //       Default::default(),
  //       |fk_info_rows: &Vec<&ForeignKeyInformationRow>| -> HashMap<String, PsqlForeignKey> {
  //         return fk_info_rows
  //           .iter()
  //           .map(|row| {
  //             return (
  //               row.constraint_name.clone(),
  //               PsqlForeignKey {
  //                 name: row.constraint_name.clone(),
  //                 column: PsqlTableColumn {
  //                   name: row.column_name.clone(),
  //                   data_type: row.column_data_type.clone(),
  //                 },
  //                 foreign_table_schema: row.table_schema.clone(),
  //                 foreign_table_name: row.table_name.clone(),
  //               },
  //             );
  //           })
  //           .collect();
  //       },
  //     );
  // }
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
          foreign_column_data_type: "integer".into(),
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

      let mut psql_tables: HashMap<TableName, PsqlTable> = HashMap::new();

      // TODO: Need to prefil psql tables

      psql_table_map_from_foreign_key_info_rows(&mut psql_tables, &fk_info_rows);

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

// #[derive(Debug)]
// struct AnyTypeToString<'a> {
//   raw: &'a [u8],
//   pub val: String,
// }
//
// impl<'a> postgres::types::FromSql<'a> for AnyTypeToString<'a> {
//   fn from_sql(
//     ty: &postgres::types::Type,
//     raw: &'a [u8],
//   ) -> Result<Self, Box<dyn std::error::Error + Sync + Send>> {
//     return postgres_protocol::types::text_from_sql(raw)
//       .map(ToString::to_string)
//       .map(|val| {
//         return AnyTypeToString { val, raw };
//       });
//   }
//
//   fn accepts(ty: &types::Type) -> bool {
//     return true;
//   }
// }
//
//
// select * from order_items where order_id in(select tagged_model_id from tagged_models where tag_id in(92, 93));
//
//
//

// with table_with_fk as (
//     SELECT
//       tc.constraint_name,
//       tc.table_schema,
//       tc.table_name,
//       kcu.column_name,
//       c.data_type AS column_data_type,
//       ccu.table_schema AS foreign_table_schema,
//       ccu.table_name AS foreign_table_name,
//       ccu.column_name AS foreign_column_name
//     FROM
//       information_schema.table_constraints AS tc
//         JOIN information_schema.key_column_usage AS kcu
//           ON tc.constraint_name = kcu.constraint_name
//           AND tc.table_schema = kcu.table_schema
//         JOIN information_schema.constraint_column_usage AS ccu
//           ON ccu.constraint_name = tc.constraint_name
//           AND ccu.table_schema = tc.table_schema
//         JOIN information_schema.columns as c
//           ON c.table_schema = tc.table_schema
//           AND c.table_name = tc.table_name
//           AND c.column_name = kcu.column_name
//     WHERE tc.constraint_type = 'FOREIGN KEY'
// )
// select table_schema, table_name from information_schema.tables as t
//   where
//     t.table_name not in (select table_name from table_with_fk) and
//     t.table_schema not in('pg_catalog', 'information_schema');

// with table_with_fk as (
//      SELECT
//        tc.constraint_name,
//        tc.table_schema,
//        tc.table_name,
//        kcu.column_name as primary_column_name,
//        c.data_type AS primary_column_data_type
//      FROM
//        information_schema.table_constraints AS tc
//          JOIN information_schema.key_column_usage AS kcu
//            ON tc.constraint_name = kcu.constraint_name
//            AND tc.table_schema = kcu.table_schema
//          JOIN information_schema.columns as c
//            ON c.table_schema = tc.table_schema
//            AND c.table_name = tc.table_name
//            AND c.column_name = kcu.column_name
//      WHERE tc.constraint_type = 'PRIMARY KEY' and
//       tc.table_schema not in ('pg_catalog', 'information_schema')
//  )
//

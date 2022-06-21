use std::collections::HashMap;
use std::rc::Rc;

use anyhow::anyhow;
use itertools::Itertools;
use postgres::types::ToSql;
use postgres::Row;

use crate::common::rose_tree::RoseTreeNode;
use crate::common::types::ResultAnyError;
use crate::db::psql::connection::PsqlConnection;
use crate::db::psql::dto::*;

type PsqlParamValue = Box<dyn ToSql + Sync>;

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
  connection: PsqlConnection,
}

impl Query {
  fn fetch_fk_info(&mut self, _schema: String) -> ResultAnyError<Vec<ForeignKeyInformationRow>> {
    // First try to build the UML for all of the tables
    // we'll query from psql information_schema tables.
    let rows: Vec<Row> = self.connection.get().query(TABLE_WITH_FK_QUERY, &[])?;

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

  fn get_table_by_name<'a, 'b>(&'a mut self) -> ResultAnyError<HashMap<String, PsqlTable<'b>>> {
    let rows: Vec<Row> = self.connection.get().query(
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

        return (psql_table.name.to_string(), psql_table);
      })
      .collect();

    return Ok(psql_table_by_name);
  }

  fn find_rows(&mut self, input: &FetchRowInput) -> ResultAnyError<Vec<Row>> {
    let query_str = format!(
      "SELECT * FROM {} where {} = $1",
      input.table_name, input.column_name
    );

    let connection = self.connection.get();
    let statement = connection.prepare(&query_str)?;

    return connection
      .query(&statement, &[input.column_value.as_ref()])
      .map_err(anyhow::Error::from);
  }

  fn get_column(
    &mut self,
    schema: &str,
    table_name: &str,
    column_name: &str,
  ) -> ResultAnyError<Option<PsqlTableColumn>> {
    let query_str =
      "SELECT * FROM information_schema.columns where table_schema = $1 and table_name = $2 and column_name = $3";

    let connection = self.connection.get();
    let statement = connection.prepare(&query_str)?;

    let row = connection
      .query_one(
        &statement,
        &[
          &schema.to_string(),
          &table_name.to_string(),
          &column_name.to_string(),
        ],
      )
      .map_err(anyhow::Error::from)?;

    let column = PsqlTableColumn::new(column_name.to_string(), row.get("data_type"));

    return Ok(Some(column));
  }
}

pub struct RelationFetcher {
  query: Query,
}

impl RelationFetcher {
  pub fn new(psql_connection: PsqlConnection) -> RelationFetcher {
    return RelationFetcher {
      query: Query {
        connection: psql_connection,
      },
    };
  }
}

pub struct FetchRowsAsRoseTreeInput<'a> {
  pub schema: &'a str,
  pub table_name: &'a str,
  pub column_name: &'a str,
  pub column_value: &'a str,
}

pub struct FetchRowInput<'a> {
  pub schema: &'a str,
  pub table_name: &'a str,
  pub column_name: &'a str,
  pub column_value: PsqlParamValue,
}

impl<'b> FetchRowInput<'b> {
  fn psql_param_value<'a>(
    column_value: String,
    column: PsqlTableColumn<'a>,
  ) -> ResultAnyError<PsqlParamValue> {
    let data_type: String = column.data_type.to_string();
    let mut value: PsqlParamValue = Box::new(column_value.clone());

    if data_type == "integer" {
      value = Box::new(column_value.clone().parse::<i32>()?);
    }

    return Ok(value);
  }
}

impl RelationFetcher {
  fn get_id_from_row(row: &Row, id_column_spec: &PsqlTableColumn) -> PsqlParamValue {
    if id_column_spec.data_type == "integer" {
      return Box::new(row.get::<_, i32>(id_column_spec.name.as_ref()));
    }

    if id_column_spec.data_type == "uuid" {
      return Box::new(row.get::<_, Uuid>(id_column_spec.name.as_ref()));
    }

    return Box::new(row.get::<_, String>(id_column_spec.name.as_ref()));
  }

  pub fn load_table_structure<'a, 'b>(
    &'a mut self,
    schema: String,
  ) -> ResultAnyError<HashMap<String, PsqlTable<'b>>> {
    let fk_info_rows = self.query.fetch_fk_info(schema)?;

    let mut table_by_name = self.query.get_table_by_name()?;

    psql_table_map_from_foreign_key_info_rows(&mut table_by_name, &fk_info_rows);

    return Ok(table_by_name);
  }

  fn find_one_row(&mut self, input: &FetchRowInput) -> ResultAnyError<Option<Row>> {
    let rows_result = self.query.find_rows(input);

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

  pub fn fetch_rose_trees_to_be_inserted<'a>(
    &mut self,
    input: &'a FetchRowsAsRoseTreeInput,
    psql_table_by_name: &'a HashMap<String, PsqlTable<'a>>,
  ) -> ResultAnyError<Vec<RoseTreeNode<PsqlTableRows<'a>>>> {
    let psql_table = psql_table_by_name.get(&input.table_name.to_string());

    if psql_table.is_none() {
      return Ok(vec![]);
    }

    let column: PsqlTableColumn = self
      .query
      .get_column(input.schema, input.table_name, input.column_name)?
      .ok_or(anyhow!("Could not find column {}", input.column_name))?;

    let mut find_root_input = input.clone();

    let fetch_row_input = FetchRowInput {
      schema: input.schema,
      table_name: input.table_name,
      column_name: input.column_name,
      column_value: FetchRowInput::psql_param_value(input.column_value.to_string(), column)?,
    };

    let row: Row = self.find_one_row(&fetch_row_input)?.ok_or_else(|| {
      anyhow!(
        "Could not find row {} {:#?} in table {}",
        input.column_name,
        input.column_value,
        input.table_name
      )
    })?;

    let row: Rc<Row> = Rc::new(row);

    // We just fetched it, let's just assume naively that the table
    // will still exist right after we fetch it.
    let current_table: PsqlTable = psql_table_by_name
      .get(&input.table_name.to_string())
      .ok_or_else(|| format!("Could not get table {}", input.table_name))
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

    let psql_table_rows: PsqlTableRows = PsqlTableRows {
      table: current_table.clone(),
      rows: vec![row.clone()],
    };

    let mut row_node: RoseTreeNode<PsqlTableRows<'a>> = RoseTreeNode::new(psql_table_rows);

    let mut fetched_table_by_name: HashMap<String, PsqlTable> = Default::default();

    let row_node_with_parents = self.fetch_referencing_rows(
      current_table.clone(),
      RelationFetcher::get_id_from_row(row.as_ref(), &current_table.primary_column),
      psql_table_by_name,
      &mut fetched_table_by_name,
    )?;

    if row_node_with_parents.is_some() {
      row_node.parents = row_node_with_parents.unwrap().parents;
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

    // Reset for current table bcs we're doing double fetch here
    fetched_table_by_name.remove(&current_table.name.to_string());

    let row_node_with_children = self.fetch_referenced_rows(
      current_table.clone(),
      &current_table.primary_column,
      RelationFetcher::get_id_from_row(row.as_ref(), &current_table.primary_column),
      psql_table_by_name,
      &mut fetched_table_by_name,
    )?;

    if row_node_with_children.is_some() {
      row_node.children = row_node_with_children.unwrap().children;
    }

    return Ok(vec![row_node]);
  }

  fn fetch_referencing_rows<'a>(
    &mut self,
    table: PsqlTable<'a>,
    id: PsqlParamValue,
    psql_table_by_name: &'a HashMap<String, PsqlTable<'a>>,
    fetched_table_by_name: &mut HashMap<String, PsqlTable<'a>>,
  ) -> ResultAnyError<Option<RoseTreeNode<PsqlTableRows<'a>>>> {
    if fetched_table_by_name.contains_key(&table.name.to_string()) {
      return Ok(None);
    }

    fetched_table_by_name.insert(table.name.to_string(), table.clone());

    let mut row_node =
      self.create_initial_node_from_row(table.clone(), &table.primary_column.name, id)?;

    if row_node.value.rows.is_empty() {
      return Ok(None);
    }

    // We  know we'll always have that 1 row
    let row = row_node.value.rows.get(0).unwrap();

    // This method should be called from lower level, so we just need to go to upper level
    let parents: Vec<RoseTreeNode<PsqlTableRows>> = table
      .referencing_fk_by_constraint_name
      .iter()
      .filter_map(|(_key, psql_foreign_key)| {
        return self
          .fetch_referencing_rows(
            psql_table_by_name[&psql_foreign_key.foreign_table_name.to_string()].clone(),
            RelationFetcher::get_id_from_row(row.as_ref(), &psql_foreign_key.column),
            psql_table_by_name,
            fetched_table_by_name,
          )
          .unwrap(); // TODO handle gracefully, convert Vec<Result<E, T>> to Result<Vec<T>, E>
      })
      .collect();

    row_node.set_parents(parents);

    return Ok(Some(row_node));
  }

  fn fetch_referenced_rows<'a>(
    &mut self,
    table: PsqlTable<'a>,
    fk_column: &PsqlTableColumn,
    id: PsqlParamValue,
    psql_table_by_name: &'a HashMap<String, PsqlTable<'a>>,
    fetched_table_by_name: &mut HashMap<String, PsqlTable<'a>>,
  ) -> ResultAnyError<Option<RoseTreeNode<PsqlTableRows<'a>>>> {
    if fetched_table_by_name.contains_key(&table.name.to_string()) {
      return Ok(None);
    }

    fetched_table_by_name.insert(table.name.to_string(), table.clone());

    let mut row_node = self.create_initial_node_from_row(table.clone(), &fk_column.name, id)?;

    if row_node.value.rows.is_empty() {
      return Ok(None);
    }

    // We  know we'll always have that 1 row
    let row = &row_node.value.rows.get(0).unwrap().clone();

    // This method should be called from oower level, so we just need to go to upper level
    let parents: Vec<RoseTreeNode<PsqlTableRows>> = table
      .referencing_fk_by_constraint_name
      .iter()
      .filter_map(|(_key, psql_foreign_key)| {
        return self
          .fetch_referencing_rows(
            psql_table_by_name[&psql_foreign_key.foreign_table_name.to_string()].clone(),
            RelationFetcher::get_id_from_row(row, &psql_foreign_key.column),
            psql_table_by_name,
            fetched_table_by_name,
          )
          .unwrap();
      })
      .collect();

    row_node.set_parents(parents);

    let primary_column = table.primary_column.clone();

    let children: Vec<RoseTreeNode<PsqlTableRows>> = table
      .referenced_fk_by_constraint_name
      .iter()
      .filter_map(|(_key, psql_foreign_key)| {
        return self
          .fetch_referenced_rows(
            psql_table_by_name[&psql_foreign_key.foreign_table_name.to_string()].clone(),
            &psql_foreign_key.column,
            RelationFetcher::get_id_from_row(row, &primary_column),
            psql_table_by_name,
            fetched_table_by_name,
          )
          .unwrap();
      })
      .collect();

    row_node.set_children(children);

    return Ok(Some(row_node));
  }

  fn create_initial_node_from_row<'a>(
    &mut self,
    table: PsqlTable<'a>,
    column_name: &str,
    id: PsqlParamValue,
  ) -> ResultAnyError<RoseTreeNode<PsqlTableRows<'a>>> {
    let rows = self.query.find_rows(&FetchRowInput {
      schema: table.schema.as_ref(),
      table_name: table.name.as_ref(),
      column_name,
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
    let referencing_fk_rows = fk_info_rows_by_table_name.get(&table_name.to_string());

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

    let referenced_fk_rows = fk_info_rows_by_foreign_table_name.get(&table_name.to_string());

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

  impl<'a> PsqlTable<'a> {
    fn basic<S>(schema: S, name: S, primary_column: PsqlTableColumn<'a>) -> PsqlTable
    where
      S: Into<Cow<'a, str>>,
    {
      return PsqlTable {
        schema: schema.into(),
        name: name.into(),
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
    use crate::common::string::s;

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

      let mut psql_table_by_name: HashMap<String, PsqlTable> = hashmap_literal! {
        s("stores") => PsqlTable::basic("public", "stores", PsqlTableColumn{
          name: "id".into(),
          data_type: "integer".into(),
        }),
        s("orders") => PsqlTable::basic("public", "orders", PsqlTableColumn{
          name: "id".into(),
          data_type: "integer".into(),
        }),
        s("order_items") => PsqlTable::basic("public", "order_items", PsqlTableColumn{
          name: "id".into(),
          data_type: "integer".into(),
        }),
        s("order_statuses") => PsqlTable::basic("public", "order_statuses", PsqlTableColumn{
          name: "id".into(),
          data_type: "integer".into(),
        }),
        s("products") => PsqlTable::basic("public", "products", PsqlTableColumn{
          name: "id".into(),
          data_type: "integer".into(),
        }),
        s("product_images") => PsqlTable::basic("public", "product_images", PsqlTableColumn{
          name: "id".into(),
          data_type: "integer".into(),
        }),
        s("product_stock_ledgers") => PsqlTable::basic("public", "product_stock_ledgers", PsqlTableColumn{
          name: "id".into(),
          data_type: "integer".into(),
        }),
        s("store_customers") => PsqlTable::basic("public", "store_customers", PsqlTableColumn{
          name: "id".into(),
          data_type: "integer".into(),
        }),
        s("store_staffs_stores") => PsqlTable::basic("public", "store_staffs_stores", PsqlTableColumn{
          name: "id".into(),
          data_type: "uuid".into(),
        }),
        s("store_staff_roles") => PsqlTable::basic("public", "store_staff_roles", PsqlTableColumn{
          name: "id".into(),
          data_type: "uuid".into(),
        }),
        s("store_staffs") => PsqlTable::basic("public", "store_staffs", PsqlTableColumn{
          name: "id".into(),
          data_type: "integer".into(),
        }),
      };

      // TODO: Need to prefil psql tables
      psql_table_map_from_foreign_key_info_rows(&mut psql_table_by_name, &fk_info_rows);

      // Make sure relations are set correctly
      // -------------------------------------------
      // table: order_items
      let order_items_table: &PsqlTable = psql_table_by_name.get("order_items").unwrap();

      assert_eq!(order_items_table.name, "order_items");
      assert_eq!(order_items_table.referencing_fk_by_constraint_name.len(), 2);
      assert_eq!(order_items_table.referenced_fk_by_constraint_name.len(), 0);

      let fk_to_orders_table_from_order_items = order_items_table
        .referencing_fk_by_constraint_name
        .get("order_items_order_id_foreign");

      assert!(fk_to_orders_table_from_order_items.is_some());

      // table: store_staffs_stores
      let store_staffs_stores_table: &PsqlTable = psql_table_by_name
        .get("store_staffs_stores")
        .ok_or_else(|| "could not get store_staffs_stores")
        .unwrap();
      assert_eq!(store_staffs_stores_table.name, "store_staffs_stores");
      assert_eq!(
        store_staffs_stores_table
          .referencing_fk_by_constraint_name
          .len(),
        3
      );

      // table: store_staffs_stores
      let products_table: &PsqlTable = psql_table_by_name.get("products").unwrap();
      assert_eq!(products_table.name, "products");
      assert_eq!(products_table.referencing_fk_by_constraint_name.len(), 1);
      assert_eq!(products_table.referenced_fk_by_constraint_name.len(), 3);

      // Make sure created tables have equal size
      // with unique table names in fk info rows
      // -------------------------------------------
      let _available_tables: HashSet<&String> =
        fk_info_rows.iter().map(|row| &row.table_name).collect();

      assert_eq!(psql_table_by_name.len(), 11)
    }
  }
}

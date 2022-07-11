use std::collections::HashMap;
use std::rc::Rc;

use postgres::Row;

use crate::common::rose_tree::RoseTreeNode;
use crate::common::types::ResultAnyError;
use crate::db::psql::dto::*;
use crate::db::psql::table_metadata::{PsqlParamValue, RowUtil, TableMetadata};

pub struct RelationFetcher {
  table_metadata: TableMetadata,
}

impl RelationFetcher {
  pub fn new(table_metadata: TableMetadata) -> RelationFetcher {
    return RelationFetcher { table_metadata };
  }
}

pub struct FetchRowsAsRoseTreeInput<'a> {
  pub schema: &'a str,
  pub table_name: &'a str,
  pub column_name: &'a str,
  pub column_value: &'a str,
}

impl RelationFetcher {
  pub fn fetch_rose_trees_to_be_inserted<'a>(
    &mut self,
    input: FetchRowsAsRoseTreeInput,
    psql_table_by_name: &'a HashMap<String, PsqlTable<'a>>,
  ) -> ResultAnyError<Vec<RoseTreeNode<PsqlTableRows<'a>>>> {
    let psql_table = psql_table_by_name.get(&input.table_name.to_string());

    if psql_table.is_none() {
      return Ok(vec![]);
    }

    let psql_table: &PsqlTable = psql_table.unwrap();

    let row: Row =
      self
        .table_metadata
        .get_one_row(psql_table, input.column_name, input.column_value)?;

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

    let mut row_node: RoseTreeNode<PsqlTableRows> = RoseTreeNode::new(psql_table_rows);

    let mut fetched_table_by_name: HashMap<String, PsqlTable> = Default::default();

    let row_node_with_parents = self.fetch_referencing_rows(
      current_table.clone(),
      RowUtil::get_id_from_row(row.as_ref(), &current_table.primary_column),
      &psql_table_by_name,
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
      RowUtil::get_id_from_row(row.as_ref(), &current_table.primary_column),
      &psql_table_by_name,
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
            RowUtil::get_id_from_row(row.as_ref(), &psql_foreign_key.column),
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
            RowUtil::get_id_from_row(row, &psql_foreign_key.column),
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
            RowUtil::get_id_from_row(row, &primary_column),
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
    return self
      .table_metadata
      .get_psql_table_rows(table, column_name, &id)
      .map(RoseTreeNode::new);
  }
}

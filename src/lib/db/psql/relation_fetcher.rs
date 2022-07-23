use std::collections::HashMap;
use std::rc::Rc;

use postgres::Row;

use crate::common::rose_tree::RoseTreeNode;
use crate::common::types::ResultAnyError;
use crate::db::psql::dto::*;
use crate::db::psql::table_metadata::TableMetadata;
use crate::db::psql::table_metadata::{PsqlParamValue, RowUtil};

pub struct RelationFetcher {
  table_metadata: Box<dyn TableMetadata>,
}

impl RelationFetcher {
  pub fn new(table_metadata: Box<dyn TableMetadata>) -> RelationFetcher {
    return RelationFetcher { table_metadata };
  }
}

pub struct FetchRowsAsRoseTreeInput<'a> {
  pub table_id: &'a PsqlTableIdentity,
  pub column_name: &'a str,
  pub column_value: &'a str,
}

impl RelationFetcher {
  pub fn fetch_rose_trees_to_be_inserted<'a>(
    &mut self,
    input: FetchRowsAsRoseTreeInput,
    psql_table_by_id: &'a HashMap<PsqlTableIdentity, PsqlTable>,
  ) -> ResultAnyError<Vec<RoseTreeNode<PsqlTableRows>>> {
    let psql_table = psql_table_by_id.get(&input.table_id);

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
    let current_table: PsqlTable = psql_table_by_id
      .get(&input.table_id)
      .ok_or_else(|| format!("Could not get table {}", input.table_id))
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

    let mut fetched_table_by_id: HashMap<PsqlTableIdentity, PsqlTable> = Default::default();

    let row_node_with_parents = self.fetch_referencing_rows(
      current_table.clone(),
      RowUtil::get_id_from_row(row.as_ref(), &current_table.primary_column),
      &psql_table_by_id,
      &mut fetched_table_by_id,
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
    fetched_table_by_id.remove(&current_table.id);

    let row_node_with_children = self.fetch_referenced_rows(
      current_table.clone(),
      &current_table.primary_column,
      RowUtil::get_id_from_row(row.as_ref(), &current_table.primary_column),
      &psql_table_by_id,
      &mut fetched_table_by_id,
    )?;

    if row_node_with_children.is_some() {
      row_node.children = row_node_with_children.unwrap().children;
    }

    return Ok(vec![row_node]);
  }

  fn fetch_referencing_rows(
    &mut self,
    table: PsqlTable,
    id: PsqlParamValue,
    psql_table_by_id: &HashMap<PsqlTableIdentity, PsqlTable>,
    fetched_table_by_id: &mut HashMap<PsqlTableIdentity, PsqlTable>,
  ) -> ResultAnyError<Option<RoseTreeNode<PsqlTableRows>>> {
    // if fetched_table_by_id.contains_key(&table.id) {
    // return Ok(None);
    // }

    fetched_table_by_id.insert(table.id.clone(), table.clone());

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
        let foreign_table_id = PsqlTableIdentity::new(
          psql_foreign_key.foreign_table_schema.clone(),
          psql_foreign_key.foreign_table_name.clone(),
        );

        return self
          .fetch_referencing_rows(
            psql_table_by_id[&foreign_table_id].clone(),
            RowUtil::get_id_from_row(row.as_ref(), &psql_foreign_key.column),
            psql_table_by_id,
            fetched_table_by_id,
          )
          .unwrap(); // TODO handle gracefully, convert Vec<Result<E, T>> to Result<Vec<T>, E>
      })
      .collect();

    row_node.set_parents(parents);

    return Ok(Some(row_node));
  }

  /// Fetch child rows, it will also populate other parents' (siblings of current node)
  /// of the current child rows
  fn fetch_referenced_rows(
    &mut self,
    table: PsqlTable,
    fk_column: &PsqlTableColumn,
    id: PsqlParamValue,
    psql_table_by_id: &HashMap<PsqlTableIdentity, PsqlTable>,
    fetched_table_by_id: &mut HashMap<PsqlTableIdentity, PsqlTable>,
  ) -> ResultAnyError<Option<RoseTreeNode<PsqlTableRows>>> {
    // if fetched_table_by_id.contains_key(&table.id) {
    // return Ok(None);
    // }

    fetched_table_by_id.insert(table.id.clone(), table.clone());

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
        let foreign_table_id = PsqlTableIdentity::new(
          psql_foreign_key.foreign_table_schema.clone(),
          psql_foreign_key.foreign_table_name.clone(),
        );

        return self
          .fetch_referencing_rows(
            psql_table_by_id[&foreign_table_id].clone(),
            RowUtil::get_id_from_row(row, &psql_foreign_key.column),
            psql_table_by_id,
            fetched_table_by_id,
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
        let foreign_table_id = PsqlTableIdentity::new(
          psql_foreign_key.foreign_table_schema.clone(),
          psql_foreign_key.foreign_table_name.clone(),
        );

        return self
          .fetch_referenced_rows(
            psql_table_by_id[&foreign_table_id].clone(),
            &psql_foreign_key.column,
            RowUtil::get_id_from_row(row, &primary_column),
            psql_table_by_id,
            fetched_table_by_id,
          )
          .unwrap();
      })
      .collect();

    row_node.set_children(children);

    return Ok(Some(row_node));
  }

  fn create_initial_node_from_row<'a>(
    &mut self,
    table: PsqlTable,
    column_name: &str,
    id: PsqlParamValue,
  ) -> ResultAnyError<RoseTreeNode<PsqlTableRows>> {
    return self
      .table_metadata
      .get_psql_table_rows(table, column_name, &id)
      .map(RoseTreeNode::new);
  }
}

#[cfg(test)]
mod test {
  use super::*;
  mod fetch_referenced_rows {
    use super::*;
    use crate::db::psql::table_metadata::MockTableMetadata;

    #[test]
    fn do_something_test() {
      let mut mock_table_metadata = Box::new(MockTableMetadata::new());
      let table = PsqlTable::new(
        "public",
        "orders",
        PsqlTableColumn::new("id", "integer"),
        Default::default(),
        Default::default(),
        Default::default(),
      );

      mock_table_metadata
        .expect_get_psql_table_rows()
        .times(1)
        .return_once(|_, _, _| {
          let table = PsqlTable::new(
            "public",
            "orders",
            PsqlTableColumn::new("id", "integer"),
            Default::default(),
            Default::default(),
            Default::default(),
          );

          Ok(PsqlTableRows {
            table,
            rows: Default::default(),
          })
        });

      let mut relation_fetcher = RelationFetcher::new(mock_table_metadata);

      let fk_column = PsqlTableColumn::new("id", "integer");

      relation_fetcher.fetch_referenced_rows(
        table,
        &fk_column,
        Box::new(1),
        &Default::default(),
        &mut Default::default(),
      );
    }
  }
}

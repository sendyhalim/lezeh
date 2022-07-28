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
  ) -> ResultAnyError<Vec<RoseTreeNode<PsqlTableRow>>> {
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

    let sink = row.get::<'_, _, FromSqlSink>("id");

    let mut row_node: RoseTreeNode<PsqlTableRow> = RoseTreeNode::new(PsqlTableRow {
      table: current_table.clone(),
      row_id_representation: sink.to_string_for_statement()?,
      row: row.clone(),
    });

    let parents = self.fetch_referencing_rows(&row_node.value, &psql_table_by_id)?;

    row_node.set_parents(parents);

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

    let children = self.fetch_referenced_rows(&row_node.value, &psql_table_by_id)?;

    row_node.set_children(children);

    return Ok(vec![row_node]);
  }

  fn fetch_referencing_rows(
    &mut self,
    current_row: &PsqlTableRow,
    psql_table_by_id: &HashMap<PsqlTableIdentity, PsqlTable>,
  ) -> ResultAnyError<Vec<RoseTreeNode<PsqlTableRow>>> {
    // This method should be called from lower level, so we just need to go to upper level
    let mut parents: Vec<RoseTreeNode<PsqlTableRow>> = Default::default();

    for (_key, psql_foreign_key) in current_row.table.referencing_fk_by_constraint_name.clone() {
      let foreign_table_id = PsqlTableIdentity::new(
        psql_foreign_key.foreign_table_schema.clone(),
        psql_foreign_key.foreign_table_name.clone(),
      );

      let foreign_table = psql_table_by_id[&foreign_table_id].clone();

      let mut parents_per_fk = self.fetch_rows_as_rose_trees(
        foreign_table.clone(),
        &foreign_table.primary_column.name,
        &RowUtil::get_id_from_row(&current_row.row, &psql_foreign_key.column),
      )?;

      for parent_row in parents_per_fk.iter_mut() {
        let grand_parents = self
          .fetch_referencing_rows(&parent_row.value, psql_table_by_id)
          .unwrap();

        parent_row.set_parents(grand_parents);
      }

      parents.extend(parents_per_fk.drain(..));
    }

    return Ok(parents);
  }

  /// Fetch child rows, it will also populate other parents' (siblings of current node)
  /// of the current child rows
  fn fetch_referenced_rows(
    &mut self,
    current_row: &PsqlTableRow,
    psql_table_by_id: &HashMap<PsqlTableIdentity, PsqlTable>,
  ) -> ResultAnyError<Vec<RoseTreeNode<PsqlTableRow>>> {
    let mut children: Vec<RoseTreeNode<PsqlTableRow>> = Default::default();
    let table = &current_row.table;

    for (_key, psql_foreign_key) in table.referenced_fk_by_constraint_name.clone() {
      let foreign_table_id = PsqlTableIdentity::new(
        psql_foreign_key.foreign_table_schema.clone(),
        psql_foreign_key.foreign_table_name.clone(),
      );

      let foreign_table = psql_table_by_id[&foreign_table_id].clone();

      let mut children_per_fk = self.fetch_rows_as_rose_trees(
        foreign_table.clone(),
        &psql_foreign_key.column.name,
        &RowUtil::get_id_from_row(&current_row.row, &table.primary_column),
      )?;

      for child_row in children_per_fk.iter_mut() {
        let parents = self.fetch_referencing_rows(&child_row.value, psql_table_by_id)?;

        child_row.set_parents(parents);

        let grand_children = self
          .fetch_referenced_rows(&child_row.value, psql_table_by_id)
          .unwrap();

        child_row.set_children(grand_children);
      }

      children.extend(children_per_fk.drain(..));
    }

    return Ok(children);
  }

  fn fetch_rows_as_rose_trees<'a>(
    &mut self,
    table: PsqlTable,
    column_name: &str,
    id: &PsqlParamValue,
  ) -> ResultAnyError<Vec<RoseTreeNode<PsqlTableRow>>> {
    let rows = self
      .table_metadata
      .get_rows(table.clone(), column_name, id)?;

    let rows = rows
      .into_iter()
      .map(|row| {
        let sink = row.get::<'_, _, FromSqlSink>("id");

        // TODO: NOT GOOD, find better ways
        let row_id = sink.to_string_for_statement().unwrap();

        return RoseTreeNode::new(PsqlTableRow {
          table: table.clone(),
          row_id_representation: row_id,
          row: Rc::new(row),
        });
      })
      .collect();

    return Ok(rows);
  }
}

// #[cfg(test)]
// mod test {
//   use super::*;
//   mod fetch_referenced_rows {
//     use super::*;
//     use crate::db::psql::table_metadata::MockTableMetadata;
//
//     fn create_dummy_table() -> PsqlTable {
//       return PsqlTable::new(
//         "public",
//         "orders",
//         PsqlTableColumn::new("id", "integer"),
//         Default::default(),
//         Default::default(),
//         Default::default(),
//       );
//     }
//
//     #[test]
//     fn if_children_is_empty() -> ResultAnyError<()> {
//       let mut mock_table_metadata = Box::new(MockTableMetadata::new());
//
//       mock_table_metadata
//         .expect_get_psql_table_rows()
//         .times(1)
//         .return_once(|_, _, _| {
//           Ok(PsqlTableRow {
//             row_id_representation: "foo".into(),
//             table: create_dummy_table(),
//             row: Default::default(),
//           })
//         });
//
//       let mut relation_fetcher = RelationFetcher::new(mock_table_metadata);
//
//       let fk_column = PsqlTableColumn::new("id", "integer");
//
//       let referenced_rows = relation_fetcher.fetch_referenced_rows(
//         create_dummy_table(),
//         &fk_column,
//         Box::new(1),
//         &Default::default(),
//         &mut Default::default(),
//       )?;
//
//       assert_eq!(referenced_rows, None);
//
//       return Ok(());
//     }
//   }
// }

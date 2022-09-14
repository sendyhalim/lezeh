use std::collections::HashMap;
use std::rc::Rc;

use anyhow::anyhow;
use petgraph::graph::Graph as BaseGraph;
use petgraph::graph::NodeIndex;
use petgraph::Directed as DirectedGraph;

use crate::psql::dto::*;
use crate::psql::table_metadata::TableMetadata;
use lezeh_common::types::ResultAnyError;

pub type RowGraph = BaseGraph<Rc<PsqlTableRow>, i32, DirectedGraph>;

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
  pub fn fetch_as_graphs<'a>(
    &mut self,
    input: FetchRowsAsRoseTreeInput,
    psql_table_by_id: &'a HashMap<PsqlTableIdentity, PsqlTable>,
  ) -> ResultAnyError<(RowGraph, NodeIndex)> {
    let psql_table = psql_table_by_id.get(&input.table_id);

    if psql_table.is_none() {
      return Err(anyhow!("Table {} not found", input.table_id));
    }

    let psql_table: &PsqlTable = psql_table.unwrap();

    let row: Rc<PsqlTableRow> = Rc::new(self.table_metadata.get_one_row(
      psql_table,
      input.column_name,
      input.column_value,
    )?);

    let mut row_graph: RowGraph = RowGraph::new();
    let node_index = row_graph.add_node(row.clone());
    let mut node_index_by_row: HashMap<Rc<PsqlTableRow>, NodeIndex> = Default::default();

    node_index_by_row.insert(row.clone(), node_index);

    // Fill parents but we do not need to fill our siblings bcs it's not required
    self.fill_referencing_rows(
      &mut row_graph,
      row.clone(),
      &psql_table_by_id,
      &mut node_index_by_row,
    )?;

    // Fill children and its parents
    self.fill_referenced_rows(
      &mut row_graph,
      row.clone(),
      &psql_table_by_id,
      &mut node_index_by_row,
    )?;

    return Ok((row_graph, node_index));
  }

  fn fill_referencing_rows(
    &mut self,
    row_graph: &mut RowGraph,
    current_row: Rc<PsqlTableRow>,
    psql_table_by_id: &HashMap<PsqlTableIdentity, PsqlTable>,
    node_index_by_row: &mut HashMap<Rc<PsqlTableRow>, NodeIndex>,
  ) -> ResultAnyError<()> {
    // This method should be called from lower level, so we just need to go to upper level
    for (_key, psql_foreign_key) in current_row.table.referencing_fk_by_constraint_name.clone() {
      let foreign_table_id = PsqlTableIdentity::new(
        psql_foreign_key.foreign_table_schema.clone(),
        psql_foreign_key.foreign_table_name.clone(),
      );

      let foreign_table = psql_table_by_id[&foreign_table_id].clone();

      let parents: Vec<Rc<PsqlTableRow>> = self
        .fetch_rows(
          foreign_table.clone(),
          &foreign_table.primary_column.name,
          &current_row.get_id(&psql_foreign_key.column),
        )?
        .into_iter()
        .map(Rc::new)
        .collect();

      let current_row_node_index = node_index_by_row.get(&current_row).unwrap().clone();

      for parent_row in parents.iter() {
        let parent_node_index = node_index_by_row
          .entry(parent_row.clone())
          .or_insert_with(|| row_graph.add_node(parent_row.clone()));

        row_graph.update_edge(current_row_node_index, *parent_node_index, -1);

        self.fill_referencing_rows(
          row_graph,
          parent_row.clone(),
          psql_table_by_id,
          node_index_by_row,
        )?;
      }
    }

    return Ok(());
  }

  /// Fetch child rows, it will also populate other parents' (siblings of current node)
  /// of the current child rows
  fn fill_referenced_rows(
    &mut self,
    row_graph: &mut RowGraph,
    current_row: Rc<PsqlTableRow>,
    psql_table_by_id: &HashMap<PsqlTableIdentity, PsqlTable>,
    node_index_by_row: &mut HashMap<Rc<PsqlTableRow>, NodeIndex>,
  ) -> ResultAnyError<()> {
    for (_key, psql_foreign_key) in current_row.table.referenced_fk_by_constraint_name.clone() {
      let foreign_table_id = PsqlTableIdentity::new(
        psql_foreign_key.foreign_table_schema.clone(),
        psql_foreign_key.foreign_table_name.clone(),
      );

      let foreign_table = psql_table_by_id[&foreign_table_id].clone();

      let children_per_fk: Vec<Rc<PsqlTableRow>> = self
        .fetch_rows(
          foreign_table.clone(),
          &psql_foreign_key.column.name,
          &current_row.get_id(&current_row.table.primary_column),
        )?
        .into_iter()
        .map(Rc::new)
        .collect();

      let current_row_node_index = node_index_by_row.get(&current_row).unwrap().clone();

      for child_row in children_per_fk.iter() {
        let child_node_index = node_index_by_row
          .entry(child_row.clone())
          .or_insert_with(|| row_graph.add_node(child_row.clone()));

        row_graph.update_edge(*child_node_index, current_row_node_index, -1);

        self.fill_referencing_rows(
          row_graph,
          child_row.clone(),
          psql_table_by_id,
          node_index_by_row,
        )?;

        self.fill_referenced_rows(
          row_graph,
          child_row.clone(),
          psql_table_by_id,
          node_index_by_row,
        )?;
      }
    }

    return Ok(());
  }

  fn fetch_rows<'a>(
    &mut self,
    table: PsqlTable,
    column_name: &str,
    id: &PsqlParamValue,
  ) -> ResultAnyError<Vec<PsqlTableRow>> {
    let rows = self
      .table_metadata
      .get_rows(table.clone(), column_name, id)?;

    return Ok(rows);
  }
}

// #[cfg(test)]
// mod test {
//   use super::*;
//   mod fetch_referenced_rows {
//     use super::*;
//     use crate::psql::table_metadata::MockTableMetadata;
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
//       let mut current_row = PsqlTableRow {
//         row_id_representation: "".to_string(),
//         table: create_dummy_table(),
//         row: Row::default(),
//       };
//
//       // mock_table_metadata
//       // .expect_get_psql_table_rows()
//       // .times(1)
//       // .return_once(|_, _, _| {
//       // Ok(PsqlTableRow {
//       // row_id_representation: "foo".into(),
//       // table: create_dummy_table(),
//       // row: Default::default(),
//       // })
//       // });
//
//       let mut relation_fetcher = RelationFetcher::new(Box::new(MockTableMetadata::default()));
//
//       // let fk_column = PsqlTableColumn::new("id", "integer");
//
//       let referenced_rows =
//         relation_fetcher.fetch_referenced_rows(&current_row, &Default::default())?;
//
//       assert_eq!(referenced_rows, vec![]);
//
//       return Ok(());
//     }
//   }
// }

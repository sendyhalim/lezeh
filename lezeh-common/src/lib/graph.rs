use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::Hash;

use petgraph::graph::Graph;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Directed;
use petgraph::Direction;

struct NodesByLevel<'a, T> {
  visited: HashSet<NodeIndex>,
  nodes_by_level: HashMap<i32, HashSet<&'a T>>,
}

impl<'a, T> std::fmt::Debug for NodesByLevel<'a, T>
where
  T: std::fmt::Debug,
{
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    let nodes_by_level = &self.nodes_by_level;
    let mut keys: Vec<&i32> = nodes_by_level.keys().into_iter().collect();

    keys.sort();

    for key in keys.into_iter() {
      let nodes: Vec<_> = nodes_by_level.get(key).unwrap().iter().collect();

      write!(f, "-----------------\n")?;
      write!(f, "{}\n", key)?;
      write!(f, "-----------------\n")?;

      for node in nodes.iter() {
        write!(f, "{:?}\n", node)?;
      }

      write!(f, "\n")?;
    }

    return Ok(());
  }
}

impl<'a, T> NodesByLevel<'a, T> {
  fn new() -> NodesByLevel<'a, T> {
    return NodesByLevel {
      visited: Default::default(),
      nodes_by_level: Default::default(),
    };
  }
}

impl<'a, T> NodesByLevel<'a, T> {
  fn fill_nodes_by_level(
    &mut self,
    graph: &'a Graph<T, i32, Directed>,
    node_index: NodeIndex,
    current_level: i32,
  ) where
    T: Hash + Eq,
  {
    // Check if node index exists here;
    if graph.node_weight(node_index).is_none() {
      return;
    }

    let node = &graph[node_index];

    self
      .nodes_by_level
      .entry(current_level)
      .or_insert_with(|| HashSet::new())
      .insert(node);

    // Children
    let mut child_indices = graph.edges_directed(node_index, Direction::Incoming);

    while let Some(edge) = child_indices.next() {
      let target_index = edge.source();

      if self.visited.insert(target_index) {
        self.fill_nodes_by_level(graph, target_index, current_level + 1);
      }
    }

    // Parents
    let mut parent_indices = graph.edges_directed(node_index, Direction::Outgoing);

    while let Some(edge) = parent_indices.next() {
      let target_index = edge.target();

      if self.visited.insert(target_index) {
        self.fill_nodes_by_level(graph, target_index, current_level - 1);
      }
    }
  }
}

/// Ergonomic method to create a hashmap that represents nodes by level
pub fn create_nodes_by_level<'a, T>(
  graph: &'a Graph<T, i32, Directed>,
  node_index: NodeIndex,
  current_level: i32,
) -> HashMap<i32, HashSet<&'a T>>
where
  T: Hash + Eq,
{
  let mut nodes_by_level = NodesByLevel::new();

  nodes_by_level.fill_nodes_by_level(graph, node_index, current_level);

  return nodes_by_level.nodes_by_level;
}

#[cfg(test)]
mod test {
  use super::*;
  mod nodes_by_level {
    use super::*;
    use lezeh_common::macros::hashmap_literal;

    #[test]
    fn test_empty_graph() {
      let graph: Graph<i32, i32> = Graph::new();

      let nodes_by_level = create_nodes_by_level(&graph, NodeIndex::new(0), 0);

      assert_eq!(nodes_by_level.is_empty(), true);
    }

    #[test]
    fn test_1_level_graph() {
      let mut graph: Graph<(i32, &str), i32> = Graph::new();

      // Just for contention and ease of read we're
      // going to use this convention -> ({level}, {label}).
      let current_index = graph.add_node((0, "a"));

      let mut nodes_by_level = create_nodes_by_level(&graph, current_index, 0);

      assert_eq!(nodes_by_level.len(), 1);
      assert_eq!(
        nodes_by_level
          .remove(&0)
          .unwrap()
          .into_iter()
          .collect::<Vec<_>>(),
        vec![&(0 as i32, "a")]
      );
    }

    #[test]
    fn test_only_has_parents() {
      let mut graph: Graph<(i32, &str), i32> = Graph::new();

      // Just for contention and ease of read we're
      // going to use this convention -> ({level}, {label}).
      let current_index = graph.add_node((0, "a"));
      let pa1 = graph.add_node((-1, "pa1"));
      let pa2 = graph.add_node((-1, "pa2"));
      let paa1 = graph.add_node((-2, "paa1"));

      graph.extend_with_edges(&vec![
        (current_index, pa1),
        (current_index, pa2),
        (pa1, paa1),
      ]);

      let nodes_by_level = create_nodes_by_level(&graph, current_index, 0);

      let expected_levels: HashMap<i32, HashSet<&(i32, &str)>> = hashmap_literal! {
        -2 => HashSet::from([&(-2, "paa1")]),
        -1 => HashSet::from([&(-1, "pa1"), &(-1, "pa2")]),
        0 => HashSet::from([&(0, "a")]),
      };

      assert_eq!(nodes_by_level.len(), 3);
      assert_eq!(nodes_by_level, expected_levels);
    }

    #[test]
    fn test_only_has_children() {
      let mut graph: Graph<(i32, &str), i32> = Graph::new();

      // Just for convention and ease of read we're
      // going to use this convention -> ({level}, {label}).
      let current_index = graph.add_node((0, "a"));
      let ca1 = graph.add_node((1, "ca1"));
      let ca2 = graph.add_node((1, "ca2"));
      let caa1 = graph.add_node((2, "caa1"));

      graph.extend_with_edges(&vec![
        (ca1, current_index),
        (ca2, current_index),
        (caa1, ca1),
      ]);

      let nodes_by_level = create_nodes_by_level(&graph, current_index, 0);

      let expected_levels: HashMap<i32, HashSet<&(i32, &str)>> = hashmap_literal! {
        2 => HashSet::from([&(2, "caa1")]),
        1 => HashSet::from([&(1, "ca1"), &(1, "ca2")]),
        0 => HashSet::from([&(0, "a")]),
      };

      assert_eq!(nodes_by_level.len(), 3);
      assert_eq!(nodes_by_level, expected_levels);
    }

    #[test]
    fn test_multi_level_graph() {
      let mut graph: Graph<(i32, &str), i32> = Graph::new();

      // Just for convention and ease of read we're
      // going to use this convention -> ({level}, {label}).
      let current_index = graph.add_node((0, "a"));
      let paa1 = graph.add_node((-2, "paa1"));
      let pa1 = graph.add_node((-1, "pa1"));
      let pa2 = graph.add_node((-1, "pa2"));
      let ca1 = graph.add_node((1, "ca1"));
      let ca2 = graph.add_node((1, "ca2"));
      let caa1 = graph.add_node((2, "caa1"));

      graph.extend_with_edges(&vec![
        // Parents
        (current_index, pa1),
        (current_index, pa2),
        (pa1, paa1),
        // Children
        (ca1, current_index),
        (ca2, current_index),
        (caa1, ca1),
      ]);

      let nodes_by_level = create_nodes_by_level(&graph, current_index, 0);

      let expected_levels: HashMap<i32, HashSet<&(i32, &str)>> = hashmap_literal! {
        -2 => HashSet::from([&(-2, "paa1")]),
        -1 => HashSet::from([&(-1, "pa1"), &(-1, "pa2")]),
        0 => HashSet::from([&(0, "a")]),
        1 => HashSet::from([&(1, "ca1"), &(1, "ca2")]),
        2 => HashSet::from([&(2, "caa1")]),
      };

      assert_eq!(nodes_by_level.len(), 5);
      assert_eq!(nodes_by_level, expected_levels);
    }
  }
}

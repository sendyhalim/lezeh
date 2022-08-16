use std::collections::HashMap;
use std::collections::HashSet;
use std::hash::Hash;

use petgraph::graph::Graph;
use petgraph::graph::NodeIndex;
use petgraph::visit::EdgeRef;
use petgraph::Directed;
use petgraph::Direction;

pub struct NodesByLevel<'a, T> {
  pub visited: HashSet<NodeIndex>,
  pub nodes_by_level: HashMap<i32, HashSet<&'a T>>,
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
  pub fn fill_nodes_by_level(
    &mut self,
    graph: &'a Graph<T, i32, Directed>,
    node_index: NodeIndex,
    current_level: i32,
  ) where
    T: Hash + Eq,
  {
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

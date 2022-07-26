use std::collections::HashMap;
use std::collections::HashSet;
use std::collections::VecDeque;
use std::fmt::Debug as DebugTrait;
use std::hash::Hash;

/// RoseTreeNode can start from multiple roots
#[derive(Debug, Hash, Clone, Eq)]
pub struct RoseTreeNode<T>
where
  T: Clone + DebugTrait + Eq + Hash,
{
  pub parents: Vec<RoseTreeNode<T>>,
  pub children: Vec<RoseTreeNode<T>>,
  pub value: T,
}

impl<T> RoseTreeNode<T>
where
  T: Clone + DebugTrait + Eq + Hash,
{
  pub fn set_parents(&mut self, parents: Vec<RoseTreeNode<T>>) {
    self.parents = parents;
  }

  pub fn set_children(&mut self, children: Vec<RoseTreeNode<T>>) {
    self.children = children;
  }

  pub fn get_value(self) -> T {
    return self.value;
  }
}

impl<T> PartialEq for RoseTreeNode<T>
where
  T: Eq + Clone + DebugTrait + Hash,
{
  fn eq(&self, other: &Self) -> bool {
    return self.value == other.value
      && self.parents == other.parents
      && self.children == other.children;
  }
}

impl<T> RoseTreeNode<T>
where
  T: Clone + DebugTrait + Eq + Hash,
{
  pub fn new(value: T) -> RoseTreeNode<T> {
    return RoseTreeNode {
      parents: Default::default(),
      children: Default::default(),
      value,
    };
  }

  /// This is a BFS problem
  pub fn parents_by_level(node: RoseTreeNode<T>) -> HashMap<i32, HashSet<T>> {
    let mut level: i32 = 0;

    if node.parents.is_empty() {
      return Default::default();
    }

    let mut ps_by_level: HashMap<i32, HashSet<T>> = Default::default();
    let mut deque_temp: VecDeque<RoseTreeNode<T>> = Default::default();

    deque_temp.extend(node.parents);

    while !deque_temp.is_empty() {
      level = level - 1;
      // Move parents to vec, this will replace node.parents with empty vec which is
      // expected bcs we don't care of node <-> parents relations after this
      let parents: Vec<RoseTreeNode<T>> = deque_temp.drain(..).collect();

      for mut parent in parents.into_iter() {
        deque_temp.extend(parent.parents.drain(..));

        let entry: &mut HashSet<_> = ps_by_level.entry(level).or_insert(Default::default());
        entry.insert(parent.value);
      }
    }

    return ps_by_level;
  }

  pub fn children_by_level(
    node: RoseTreeNode<T>,
    parents_by_level: &mut HashMap<i32, HashSet<T>>,
  ) -> HashMap<i32, HashSet<T>> {
    let mut level: i32 = 0;

    if node.children.is_empty() {
      return Default::default();
    }

    let mut children_by_level: HashMap<i32, HashSet<T>> = Default::default();
    let mut deque_temp: VecDeque<RoseTreeNode<T>> = Default::default();

    deque_temp.extend(node.children);

    while !deque_temp.is_empty() {
      level = level + 1;

      // Drain first the temp into a vec
      let children: Vec<RoseTreeNode<T>> = deque_temp.drain(..).collect();

      println!("level {} child {}", level, &children.len());
      for mut child in children.into_iter() {
        let parents_by_level_from_current_child: HashMap<i32, HashSet<_>> =
          RoseTreeNode::parents_by_level(child.clone());

        // Drain parents, we already put it in parents_by_level_from_current_child
        child.parents = vec![];

        // println!(
        //   "[{:#?}]Constructing parents {:#?}",
        //   child, parents_by_level_from_current_child
        // );

        for (parent_level, parents) in parents_by_level_from_current_child.into_iter() {
          parents_by_level
            .entry(level - parent_level.abs())
            .or_insert(Default::default())
            .extend(parents);
        }

        deque_temp.extend(child.children.drain(..));

        let entry: &mut HashSet<_> = children_by_level.entry(level).or_insert(Default::default());

        let inserted = entry.insert(child.value);

        if level == 2 {
          println!("Inserted {}", inserted);
        }
      }
    }

    return children_by_level;
  }

  /// Iterate to parents and children from the given node,
  /// collecting all the relations (BFS-like) and return a hahsmap
  /// where the key is the level of the hashset nodes.
  pub fn nodes_by_level(node: RoseTreeNode<T>) -> HashMap<i32, HashSet<T>> {
    let mut nodes_by_level: HashMap<i32, HashSet<_>> = Default::default();

    // Prefill current rows
    let mut current_level_rows: HashSet<T> = Default::default();
    current_level_rows.insert(node.value.clone());
    nodes_by_level.insert(0, current_level_rows);

    // Populate parents
    nodes_by_level.extend(RoseTreeNode::parents_by_level(node.clone()));

    // Populate children
    let children_by_level = RoseTreeNode::children_by_level(node.clone(), &mut nodes_by_level);
    nodes_by_level.extend(children_by_level);

    return nodes_by_level;
  }
}

#[cfg(test)]
mod test {
  use super::*;

  mod parents_by_level {
    use super::*;
    use crate::common::macros::hashmap_literal;

    #[test]
    fn it_should_load_parents() {
      let mut node = RoseTreeNode::new("level_0");
      let mut parent_a = RoseTreeNode::new("level_1_parent_a");
      let mut parent_b = RoseTreeNode::new("level_1_parent_b");

      parent_a.set_parents(vec![RoseTreeNode::new("level_2_parent_a")]);
      parent_b.set_children(vec![RoseTreeNode::new("level_1_child_b")]);

      node.set_parents(vec![parent_a, parent_b.clone()]);

      let parents_by_level = RoseTreeNode::parents_by_level(node);

      let expected_structure: HashMap<i32, HashSet<_>> = hashmap_literal! {
        -1 => vec!["level_1_parent_b", "level_1_parent_a"].into_iter().collect(),
        -2 => vec!["level_2_parent_a"].into_iter().collect(),
      };

      assert_eq!(parents_by_level, expected_structure);
    }
  }

  mod children_by_level {
    use super::*;

    mod given_empty_parents {
      use super::*;
      use crate::common::macros::hashmap_literal;

      #[test]
      fn it_should_load_children() {
        let mut node = RoseTreeNode::new("level_0");
        let mut child_a = RoseTreeNode::new("level_1_child_a");
        let mut child_b = RoseTreeNode::new("level_1_child_b");
        let mut level_2_child_a = RoseTreeNode::new("level_2_child_a");
        let level_2_child_b = RoseTreeNode::new("level_2_child_b");

        level_2_child_a.set_parents(vec![RoseTreeNode::new("level_1_parent_x")]);

        child_a.set_children(vec![level_2_child_a.clone()]);
        child_b.set_children(vec![level_2_child_b.clone()]);
        child_b.set_parents(vec![RoseTreeNode::new("level_0_parent_b")]);

        node.set_children(vec![child_a, child_b.clone()]);

        let mut parents_by_level: HashMap<i32, HashSet<&str>> = Default::default();
        let children_vec = RoseTreeNode::children_by_level(node, &mut parents_by_level);

        let expected_children_structure = hashmap_literal! {
          1 => vec!["level_1_child_a", "level_1_child_b"].into_iter().collect(),
          2 => vec!["level_2_child_a", "level_2_child_b"].into_iter().collect(),
        };

        let expected_parents_structure = hashmap_literal! {
          0 => vec!["level_0_parent_b"].into_iter().collect(),
          1 => vec!["level_1_parent_x"].into_iter().collect()
        };

        assert_eq!(children_vec, expected_children_structure);
        assert_eq!(parents_by_level, expected_parents_structure);
      }
    }

    mod given_prefilled_parents {
      use super::*;
      use crate::common::macros::hashmap_literal;

      #[test]
      fn it_should_load_parents_into_existing_levels() {
        let mut node = RoseTreeNode::new("level_0");
        let mut child_a = RoseTreeNode::new("level_1_child_a");
        let mut child_b = RoseTreeNode::new("level_1_child_b");

        let mut parent_a = RoseTreeNode::new("level_0_parent_a");
        parent_a.set_parents(vec![RoseTreeNode::new("level_1_parent_a")]);

        child_a.set_parents(vec![parent_a.clone()]);

        child_a.set_children(vec![RoseTreeNode::new("level_2_child_a")]);
        child_b.set_parents(vec![RoseTreeNode::new("level_0_parent_b")]);

        node.set_children(vec![child_a, child_b]);

        let mut parents_by_level: HashMap<i32, HashSet<&str>> = hashmap_literal! {
          -1 => vec!["level_1_parent_a"].into_iter().collect(),
          -2 => vec![
            "level_2_parent_a",
            "level_2_parent_b"
          ].into_iter().collect()
        };

        let children_vec = RoseTreeNode::children_by_level(node, &mut parents_by_level);

        let expected_children_structure = hashmap_literal! {
          1 => vec!["level_1_child_a", "level_1_child_b"].into_iter().collect(),
          2 => vec!["level_2_child_a"].into_iter().collect(),
        };

        let expected_parents_structure = hashmap_literal! {
           0 => vec!["level_0_parent_a", "level_0_parent_b"].into_iter().collect(),
          -1 => vec!["level_1_parent_a"].into_iter().collect(),
          -2 => vec!["level_2_parent_a", "level_2_parent_b"].into_iter().collect()
        };

        assert_eq!(
          children_vec, expected_children_structure,
          "not equal left {:#?} and {:#?}",
          children_vec, expected_children_structure
        );
        assert_eq!(parents_by_level, expected_parents_structure);
      }
    }
  }
}

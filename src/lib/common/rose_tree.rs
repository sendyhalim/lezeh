use std::collections::HashMap;
use std::collections::VecDeque;
use std::fmt::Debug as DebugTrait;

use crate::common::macros::hashmap_literal;

/// RoseTreeNode can start from multiple roots
#[derive(Debug, Clone)]
pub struct RoseTreeNode<T>
where
  T: Clone + DebugTrait,
{
  pub parents: Vec<RoseTreeNode<T>>,
  pub children: Vec<RoseTreeNode<T>>,
  pub value: T,
}

impl<T> RoseTreeNode<T>
where
  T: Clone + DebugTrait,
{
  pub fn set_parents(&mut self, parents: Vec<RoseTreeNode<T>>) {
    self.parents = parents;
  }

  pub fn set_children(&mut self, children: Vec<RoseTreeNode<T>>) {
    self.children = children;
  }
}

impl<T> PartialEq for RoseTreeNode<T>
where
  T: Eq + Clone + DebugTrait,
{
  fn eq(&self, other: &Self) -> bool {
    return self.value == other.value
      && self.parents == other.parents
      && self.children == other.children;
  }
}

impl<T> RoseTreeNode<T>
where
  T: Clone + DebugTrait,
{
  pub fn new(value: T) -> RoseTreeNode<T> {
    return RoseTreeNode {
      parents: Default::default(),
      children: Default::default(),
      value,
    };
  }

  /// This is a BFS problem
  pub fn parents_by_level(node: RoseTreeNode<T>) -> HashMap<i32, Vec<RoseTreeNode<T>>> {
    let mut level: i32 = 0;

    if node.parents.is_empty() {
      return Default::default();
    }

    let mut ps_by_level: HashMap<i32, Vec<RoseTreeNode<T>>> = Default::default();
    let mut deque_temp: VecDeque<RoseTreeNode<T>> = Default::default();

    deque_temp.extend(node.parents);

    while !deque_temp.is_empty() {
      level = level - 1;
      // Drain first the temp into a vec
      let temp: Vec<RoseTreeNode<T>> = deque_temp.drain(..).collect();

      // Now get all the parents for all nodes at the current level
      for mut node in temp.into_iter() {
        // Move parents to vec, this will replace node.parents with empty vec which is
        // expected bcs we don't care of node <-> parents relations after this
        let parents: Vec<RoseTreeNode<T>> = node.parents.drain(..).collect();

        for parent in parents.into_iter() {
          deque_temp.push_back(parent);
        }

        let entry: &mut Vec<_> = ps_by_level.entry(level).or_insert(Default::default());

        entry.push(node);
      }
    }

    return ps_by_level;
  }

  pub fn rose_tree_to_vec(rose_tree: RoseTreeNode<T>) {
    let parents = RoseTreeNode::parents_by_level(rose_tree);
  }

  pub fn children_by_level(
    node: RoseTreeNode<T>,
    parents_by_level: &mut HashMap<i32, Vec<RoseTreeNode<T>>>,
  ) -> HashMap<i32, Vec<RoseTreeNode<T>>> {
    let mut level: i32 = 0;

    if node.children.is_empty() {
      return Default::default();
    }

    let mut children_by_level: HashMap<i32, Vec<RoseTreeNode<T>>> = Default::default();
    let mut deque_temp: VecDeque<RoseTreeNode<T>> = Default::default();

    deque_temp.extend(node.children);

    while !deque_temp.is_empty() {
      level = level + 1;

      // Drain first the temp into a vec
      let temp: Vec<RoseTreeNode<T>> = deque_temp.drain(..).collect();

      // Now get all the parents for all nodes at the current level
      for mut node in temp.into_iter() {
        // Move parents to vec, this will replace node.parents with empty vec which is
        // expected bcs we don't care of node <-> parents relations after this
        let children: Vec<RoseTreeNode<T>> = node.children.drain(..).collect();

        for mut child in children.into_iter() {
          let parents_by_level_from_current_child: HashMap<i32, Vec<RoseTreeNode<T>>> =
            RoseTreeNode::parents_by_level(child.clone());

          // Drain parents, we already put it in parents_by_level_from_current_child
          child.parents = vec![];

          println!(
            "[{:#?}]Constructing parents {:#?}",
            child, parents_by_level_from_current_child
          );

          for (parent_level, parents) in parents_by_level_from_current_child.into_iter() {
            parents_by_level
              .entry((level + 1) - parent_level.abs())
              .or_insert(Default::default())
              .extend(parents);
          }

          deque_temp.push_back(child);
        }

        let entry: &mut Vec<_> = children_by_level.entry(level).or_insert(Default::default());

        entry.push(node);
      }
    }

    return children_by_level;
  }
}

#[cfg(test)]
mod test {
  mod parents_by_level {
    use super::super::*;

    #[test]
    fn it_should_load_parents() {
      let mut node = RoseTreeNode::new("level_0");
      let mut parent_a = RoseTreeNode::new("level_1_parent_a");
      let mut parent_b = RoseTreeNode::new("level_1_parent_b");

      parent_a.set_parents(vec![RoseTreeNode::new("level_2_parent_a")]);
      parent_b.set_children(vec![RoseTreeNode::new("level_1_child_b")]);

      node.set_parents(vec![parent_a, parent_b.clone()]);

      let parents_vec = RoseTreeNode::parents_by_level(node);

      let expected_structure = hashmap_literal! {
        -1 => vec![RoseTreeNode::new("level_1_parent_a"), parent_b.clone()],
        -2 => vec![RoseTreeNode::new("level_2_parent_a")],
      };

      assert_eq!(parents_vec, expected_structure);
    }
  }

  mod children_by_level {
    use super::super::*;

    mod given_empty_parents {
      use super::*;

      #[test]
      fn it_should_load_children() {
        let mut node = RoseTreeNode::new("level_0");
        let mut child_a = RoseTreeNode::new("level_1_child_a");
        let mut child_b = RoseTreeNode::new("level_1_child_b");
        let mut level_2_child_a = RoseTreeNode::new("level_2_child_a");

        level_2_child_a.set_parents(vec![RoseTreeNode::new("level_1_parent_x")]);

        child_a.set_children(vec![level_2_child_a.clone()]);
        child_b.set_parents(vec![RoseTreeNode::new("level_0_parent_b")]);

        node.set_children(vec![child_a, child_b.clone()]);

        let mut parents_by_level: HashMap<i32, Vec<RoseTreeNode<&str>>> = Default::default();
        let children_vec = RoseTreeNode::children_by_level(node, &mut parents_by_level);

        let expected_children_structure = hashmap_literal! {
          1 => vec![RoseTreeNode::new("level_1_child_a"), RoseTreeNode::new("level_1_child_b")],
          2 => vec![RoseTreeNode::new("level_2_child_a")],
        };

        let expected_parents_structure = hashmap_literal! {
          0 => vec![RoseTreeNode::new("level_0_parent_b")],
          1 => vec![RoseTreeNode::new("level_1_parent_x")]
        };

        assert_eq!(children_vec, expected_children_structure);
        assert_eq!(parents_by_level, expected_parents_structure);
      }
    }

    mod given_prefilled_parents {
      use super::*;

      #[test]
      fn it_should_load_parents_into_existing_levels() {
        let mut node = RoseTreeNode::new("level_0");
        let mut child_a = RoseTreeNode::new("level_1_child_a");
        let mut child_b = RoseTreeNode::new("level_1_child_b");

        let mut parent_x = RoseTreeNode::new("level_1_parent_x");
        parent_x.set_parents(vec![RoseTreeNode::new("level_2_parent_x")]);

        child_a.set_parents(vec![parent_x.clone()]);

        child_a.set_children(vec![RoseTreeNode::new("level_2_child_a")]);
        child_b.set_parents(vec![RoseTreeNode::new("level_1_parent_b")]);

        node.set_children(vec![child_a, child_b.clone()]);

        let mut parents_by_level: HashMap<i32, Vec<RoseTreeNode<&str>>> = hashmap_literal! {
          -1 => vec![RoseTreeNode::new("level_1_parent_a")],
          -2 => vec![
            RoseTreeNode::new("level_2_parent_a"),
            RoseTreeNode::new("level_2_parent_b")
          ]
        };

        let children_vec = RoseTreeNode::children_by_level(node, &mut parents_by_level);

        let expected_children_structure = hashmap_literal! {
          1 => vec![RoseTreeNode::new("level_1_child_a"), child_b.clone()],
          2 => vec![RoseTreeNode::new("level_2_child_a")],
        };

        let expected_parents_structure = hashmap_literal! {
           0 => vec![RoseTreeNode::new("level_1_parent_x")],
          -1 => vec![RoseTreeNode::new("level_1_parent_a"), RoseTreeNode::new("level_2_parent_x")],
          -2 => vec![
            RoseTreeNode::new("level_2_parent_a"),
            RoseTreeNode::new("level_2_parent_b")
          ]
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

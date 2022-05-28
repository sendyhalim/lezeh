use std::cell::RefCell;
use std::collections::HashMap;
use std::collections::VecDeque;

use crate::common::macros::hashmap_literal;

/// RoseTreeNode can start from multiple roots
#[derive(Debug, Clone)]
pub struct RoseTreeNode<T> {
  pub parents: RefCell<Vec<RoseTreeNode<T>>>,
  pub children: RefCell<Vec<RoseTreeNode<T>>>,
  pub value: T,
}

impl<T> PartialEq for RoseTreeNode<T>
where
  T: Clone + Eq,
{
  fn eq(&self, other: &Self) -> bool {
    return self.value == other.value
      && self.parents == other.parents
      && self.children == other.children;
  }
}

impl<T> RoseTreeNode<T> {
  pub fn new(value: T) -> RoseTreeNode<T> {
    return RoseTreeNode {
      parents: Default::default(),
      children: Default::default(),
      value,
    };
  }

  /// This is a BFS problem
  pub fn parents_by_level(rose_tree: RoseTreeNode<T>) -> HashMap<i32, Vec<RoseTreeNode<T>>> {
    let mut level: i32 = 0;

    // Find roots by going up

    if rose_tree.parents.borrow().is_empty() {
      // Do nothing
    }

    let mut deque: VecDeque<(i32, Vec<RoseTreeNode<T>>)> = Default::default();
    let mut deque_temp: VecDeque<RoseTreeNode<T>> = Default::default();
    deque_temp.extend(rose_tree.parents.into_inner());

    while !deque_temp.is_empty() {
      level = level - 1;
      // Drain first the temp into a vec
      deque.push_front((level, deque_temp.drain(..).collect()));

      // Now get all the parents for all nodes at the current level
      for node in deque.front().unwrap().1.iter() {
        for parent in node.parents.take().into_iter() {
          deque_temp.push_back(parent);
        }
      }
    }

    return deque.into_iter().collect::<HashMap<_, _>>();
  }

  pub fn rose_tree_to_vec(rose_tree: RoseTreeNode<T>) {
    let parents = RoseTreeNode::parents_by_level(rose_tree);
  }

  // pub fn children_to_vec(
  //   node: RoseTreeNode<T>,
  //   parents_vec: Vec<(i32, Vec<RoseTreeNode<T>>)>,
  // ) -> Vec<(i32, Vec<RoseTreeNode<T>>)> {
  //   // Find roots by going up

  //   if node.children.borrow().is_empty() {
  //     // Do nothing
  //   }

  //   let mut level: i32 = 0;

  //   let mut deque: VecDeque<Vec<RoseTreeNode<T>>> = Default::default();
  //   let mut deque_temp: VecDeque<RoseTreeNode<T>> = Default::default();
  //   deque_temp.push_back(node);

  //   // TODO:
  //   // A) Need to keep track of level
  //   // B) Need to merge parents from currently iterated child, remember
  //   // that the level is also relative from the current level. Idea: it might be easier to use
  //   // hashmap though HashMap<level, Vec<_>>
  //   while !deque_temp.is_empty() {
  //     level = level + 1;

  //     // Drain first the temp into a vec
  //     deque.push_back(deque_temp.drain(..).collect());

  //     // Now get all the children for all nodes at the current level
  //     let mut parents: Vec<Vec<RoseTreeNode<T>>> = Default::default();

  //     for node in deque.back().unwrap().iter() {
  //       for child in node.children.take().into_iter() {
  //         let parents_from_current_child = RoseTreeNode::parents_to_vec(node);
  //         parents.extend(parents_from_current_child);
  //         deque_temp.push_front(child);
  //       }
  //     }

  //     for p in parents.into_iter() {
  //       deque.push_front(p);
  //     }
  //   }

  //   return deque.into_iter().collect::<Vec<_>>();

  //   // let table_names: Vec<Vec<String>> = deque
  //   // .into_iter()
  //   // .collect::<Vec<_>>()
  //   // .into_iter()
  //   // .map(|nodes| {
  //   // return nodes
  //   // .into_iter()
  //   // .map(|node| {
  //   // return node.value.table.name.to_string();
  //   // })
  //   // .collect();
  //   // })
  //   // .collect();

  //   // println!("{:#?}", table_names);
  // }
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

      parent_a.parents = RefCell::from(vec![RoseTreeNode::new("level_2_parent_a")]);
      parent_b.children = RefCell::from(vec![RoseTreeNode::new("level_1_child_b")]);

      node.parents = RefCell::from(vec![parent_a, parent_b.clone()]);

      let parents_vec = RoseTreeNode::parents_by_level(node);

      let expected_structure = hashmap_literal! {
        -2 => vec![RoseTreeNode::new("level_2_parent_a")],
        -1 => vec![RoseTreeNode::new("level_1_parent_a"), parent_b.clone()],
      };

      assert_eq!(parents_vec, expected_structure);
    }
  }
}

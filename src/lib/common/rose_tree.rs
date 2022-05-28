use std::cell::RefCell;
use std::collections::VecDeque;

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
  pub fn parents_to_vec(rose_tree: RoseTreeNode<T>) -> Vec<(i32, Vec<RoseTreeNode<T>>)> {
    let mut level: i32 = 0;

    // Find roots by going up

    if rose_tree.parents.borrow().is_empty() {
      // Do nothing
    }

    let mut deque: VecDeque<(i32, Vec<RoseTreeNode<T>>)> = Default::default();
    let mut deque_temp: VecDeque<RoseTreeNode<T>> = Default::default();
    deque_temp.push_back(rose_tree);

    while !deque_temp.is_empty() {
      level = level + 1;
      // Drain first the temp into a vec
      deque.push_front((level, deque_temp.drain(..).collect()));

      // Now get all the parents for all nodes at the current level
      for node in deque.front().unwrap().1.iter() {
        for parent in node.parents.take().into_iter() {
          deque_temp.push_back(parent);
        }
      }
    }

    return deque.into_iter().collect::<Vec<_>>();
  }
}

#[cfg(test)]
mod test {
  mod parents_to_vec {
    use super::super::*;

    #[test]
    fn it_should_load_parents() {
      let mut node = RoseTreeNode::new("level_0");
      let mut parent_a = RoseTreeNode::new("level_1_parent_a");
      let mut parent_b = RoseTreeNode::new("level_1_parent_b");

      parent_a.parents = RefCell::from(vec![RoseTreeNode::new("level_2_parent_a")]);
      parent_b.children = RefCell::from(vec![RoseTreeNode::new("level_1_child_b")]);

      node.parents = RefCell::from(vec![parent_a, parent_b.clone()]);

      let parents_vec = RoseTreeNode::parents_to_vec(node);

      let expected_structure = vec![
        (3, vec![RoseTreeNode::new("level_2_parent_a")]),
        (
          2,
          vec![RoseTreeNode::new("level_1_parent_a"), parent_b.clone()],
        ),
        (1, vec![RoseTreeNode::new("level_0")]),
      ];

      assert_eq!(parents_vec, expected_structure);
    }
  }
}

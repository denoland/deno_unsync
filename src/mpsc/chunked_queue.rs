use std::collections::{LinkedList, VecDeque};

/// A queue that stores elements in chunks in a linked list
/// to reduce allocations.
pub(crate) struct ChunkedQueue<T> {
  chunks: LinkedList<VecDeque<T>>,
}

impl<T> Default for ChunkedQueue<T> {
  fn default() -> Self {
    Self {
      chunks: Default::default(),
    }
  }
}


impl<T> ChunkedQueue<T> {
  pub fn len(&self) -> usize {
    self.chunks.iter().map(VecDeque::len).sum()
  }

  pub fn push_back(&mut self, value: T) {
    if let Some(tail) = self.chunks.back_mut() {
      if tail.len() < tail.capacity() {
        tail.push_back(value);
        return;
      }
    }
    let mut new_buffer = VecDeque::with_capacity(1024);
    new_buffer.push_back(value);
    self.chunks.push_back(new_buffer);
  }

  pub fn pop_front(&mut self) -> Option<T> {
    if let Some(head) = self.chunks.front_mut() {
      let value = head.pop_front();
      if value.is_some() && head.is_empty() && self.chunks.len() > 1 {
        self.chunks.pop_front().unwrap();
      }
      value
    } else {
      None
    }
  }
}

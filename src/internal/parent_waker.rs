// Copyright 2018-2024 the Deno authors. MIT license.

use std::cell::RefCell;
use std::task::Waker;

#[derive(Default)]
pub struct ParentWaker(RefCell<Option<Waker>>);

impl ParentWaker {
  pub fn set(&self, waker: Waker) {
    *self.0.borrow_mut() = Some(waker);
  }

  pub fn wake(&self) {
    if let Some(waker) = self.0.borrow_mut().take() {
      waker.wake();
    }
  }

  pub fn wake_by_ref(&self) {
    if let Some(waker) = self.0.borrow_mut().as_ref() {
      waker.wake_by_ref();
    }
  }
}
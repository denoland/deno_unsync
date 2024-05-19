// Copyright 2018-2024 the Deno authors. MIT license.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::fmt::Formatter;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::Context;
use std::task::Poll;

use crate::UnsyncWaker;

pub struct SendError<T>(pub T);

impl<T> std::fmt::Debug for SendError<T>
where
  T: std::fmt::Debug,
{
  fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
    f.debug_tuple("SendError").field(&self.0).finish()
  }
}

pub struct Sender<T> {
  shared: Rc<RefCell<Shared<T>>>,
}

impl<T> Sender<T> {
  pub fn send(&self, value: T) -> Result<(), SendError<T>> {
    let mut shared = self.shared.borrow_mut();
    if shared.closed {
      return Err(SendError(value));
    }
    shared.queue.push_back(value);
    shared.waker.wake();
    Ok(())
  }
}

impl<T> Drop for Sender<T> {
  fn drop(&mut self) {
    let mut shared = self.shared.borrow_mut();
    shared.closed = true;
    shared.waker.wake();
  }
}

pub struct Receiver<T> {
  shared: Rc<RefCell<Shared<T>>>,
}

impl<T> Drop for Receiver<T> {
  fn drop(&mut self) {
    let mut shared = self.shared.borrow_mut();
    shared.closed = true;
  }
}

impl<T> Receiver<T> {
  /// Receives a value from the channel, returning `None` if there
  /// are no more items and the channel is closed.
  pub async fn recv(&mut self) -> Option<T> {
    // note: this is `&mut self` so that it can't be polled
    // concurrently. DO NOT change this to `&self` because
    // then futures will lose their wakers.
    RecvFuture {
      shared: &self.shared,
    }
    .await
  }
}

struct RecvFuture<'a, T> {
  shared: &'a RefCell<Shared<T>>,
}

impl<'a, T> Future for RecvFuture<'a, T> {
  type Output = Option<T>;

  fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    let mut shared = self.shared.borrow_mut();
    if let Some(value) = shared.queue.pop_front() {
      Poll::Ready(Some(value))
    } else if shared.closed {
      Poll::Ready(None)
    } else {
      shared.waker.register(cx.waker());
      Poll::Pending
    }
  }
}

struct Shared<T> {
  queue: VecDeque<T>,
  waker: UnsyncWaker,
  closed: bool,
}

/// A ![`Sync`] and ![`Sync`] equivalent to `tokio::sync::unbounded_channel`.
pub fn unbounded_channel<T>() -> (Sender<T>, Receiver<T>) {
  let shared = Rc::new(RefCell::new(Shared {
    queue: VecDeque::new(),
    waker: UnsyncWaker::default(),
    closed: false,
  }));
  (
    Sender {
      shared: shared.clone(),
    },
    Receiver { shared },
  )
}

#[cfg(test)]
mod test {
  use tokio::join;

  use super::*;

  #[tokio::test(flavor = "current_thread")]
  async fn sends_receives_exits() {
    let (sender, mut receiver) = unbounded_channel::<usize>();
    sender.send(1).unwrap();
    assert_eq!(receiver.recv().await, Some(1));
    sender.send(2).unwrap();
    assert_eq!(receiver.recv().await, Some(2));
    drop(sender);
    assert_eq!(receiver.recv().await, None);
  }

  #[tokio::test(flavor = "current_thread")]
  async fn sends_multiple_then_drop() {
    let (sender, mut receiver) = unbounded_channel::<usize>();
    sender.send(1).unwrap();
    sender.send(2).unwrap();
    drop(sender);
    assert_eq!(receiver.recv().await, Some(1));
    assert_eq!(receiver.recv().await, Some(2));
    assert_eq!(receiver.recv().await, None);
  }

  #[tokio::test(flavor = "current_thread")]
  async fn receiver_dropped_sending() {
    let (sender, receiver) = unbounded_channel::<usize>();
    drop(receiver);
    let err = sender.send(1).unwrap_err();
    assert_eq!(err.0, 1);
  }

  #[tokio::test(flavor = "current_thread")]
  async fn receiver_recv_then_drop_sender() {
    let (sender, mut receiver) = unbounded_channel::<usize>();
    let future = crate::task::spawn(async move {
      let value = receiver.recv().await;
      value.is_none()
    });
    let future2 = crate::task::spawn(async move {
      drop(sender);
      true
    });
    let (first, second) = join!(future, future2);
    assert!(first.unwrap());
    assert!(second.unwrap());
  }
}

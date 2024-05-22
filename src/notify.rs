use std::cell::Cell;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::Context;
use std::task::Poll;
use std::task::Waker;

#[derive(Default, Copy, Clone, PartialEq, Eq)]
enum NotifyState {
  #[default]
  Pending,
  Notified,
  Dropped,
}

struct WakerWithState {
  inner: Waker,
  notify_state: Rc<Cell<NotifyState>>,
}

impl WakerWithState {
  pub fn notify(self) {
    debug_assert!(self.notify_state.get() == NotifyState::Pending);
    self.notify_state.set(NotifyState::Notified);
    self.inner.wake();
  }

  pub fn is_future_dropped(&self) -> bool {
    self.notify_state.get() == NotifyState::Dropped
  }
}

#[derive(Default)]
pub struct NotifyInner {
  pending_notify: usize,
  wakers: VecDeque<WakerWithState>,
}

#[derive(Default)]
pub struct Notify(Rc<RefCell<NotifyInner>>);

impl Notify {
  pub fn new() -> Self {
    Self::default()
  }

  pub fn notify_one(&self) {
    let mut maybe_waker = None;
    {
      let mut inner = self.0.borrow_mut();
      while let Some(waker) = inner.wakers.pop_front() {
        if !waker.is_future_dropped() {
          maybe_waker = Some(waker);
          break;
        }
      }
      // no wakers, so make the next one be notified immediately
      if maybe_waker.is_none() {
        inner.pending_notify += 1;
      }
    }
    if let Some(waker) = maybe_waker {
      waker.notify();
    }
  }

  pub fn notified(&self) -> impl Future<Output = ()> {
    NotifiedFuture {
      inner: self.0.clone(),
      notify_state: Default::default(),
    }
  }
}

#[derive(Default)]
struct NotifiedFuture {
  inner: Rc<RefCell<NotifyInner>>,
  notify_state: Rc<Cell<NotifyState>>,
}

impl Drop for NotifiedFuture {
  fn drop(&mut self) {
    if self.notify_state.get() == NotifyState::Pending {
      self.notify_state.set(NotifyState::Dropped);
    }
  }
}

impl Future for NotifiedFuture {
  type Output = ();

  fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    match self.notify_state.get() {
      NotifyState::Pending => {
        let mut inner = self.inner.borrow_mut();
        if inner.pending_notify > 0 {
          inner.pending_notify -= 1;
          self.notify_state.set(NotifyState::Notified);
          return Poll::Ready(());
        }
        let waker = WakerWithState {
          inner: cx.waker().clone(),
          notify_state: self.notify_state.clone(),
        };
        inner.wakers.push_back(waker);
        Poll::Pending
      }
      NotifyState::Notified => Poll::Ready(()),
      NotifyState::Dropped => unreachable!(),
    }
  }
}

#[cfg(test)]
mod test {
  use super::*;

  #[tokio::test(flavor = "current_thread")]
  async fn multiple_notify_one_before_await() {
    let notify = Notify::new();
    notify.notify_one();
    notify.notify_one();
    notify.notified().await;
    notify.notified().await;
  }

  #[tokio::test(flavor = "current_thread")]
  async fn should_build_up_notifies() {
    let notify1 = Rc::new(Notify::new());
    let notify2 = Rc::new(Notify::new());
    let future = crate::spawn({
      let notify1 = notify1.clone();
      let notify2 = notify2.clone();
      async move {
        notify1.notified().await;
        notify2.notify_one();
      }
    });
    notify1.notify_one();
    notify2.notified().await;
    future.await.unwrap();
  }
}

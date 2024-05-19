// Copyright 2018-2024 the Deno authors. MIT license.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::task::Context;
use std::task::Poll;
use std::task::Waker;

use crate::Flag;

struct SemaphoreStateWaker {
  inner: Waker,
  // if the future is dropped then we don't want to wake
  // this waker, but the next one in the queue
  is_future_dropped: Rc<Flag>,
}

struct SemaphoreState {
  closed: bool,
  max_permits: usize,
  acquired_permits: usize,
  wakers: VecDeque<SemaphoreStateWaker>,
}

pub struct SemaphorePermit(Rc<Semaphore>);

impl Drop for SemaphorePermit {
  fn drop(&mut self) {
    self.0.release();
  }
}

pub struct Semaphore {
  state: RefCell<SemaphoreState>,
}

impl Semaphore {
  pub fn new(max_permits: usize) -> Self {
    Self {
      state: RefCell::new(SemaphoreState {
        closed: false,
        max_permits,
        acquired_permits: 0,
        wakers: VecDeque::new(),
      }),
    }
  }

  pub fn acquire(self: &Rc<Self>) -> impl Future<Output = Result<SemaphorePermit, ()>> {
    AcquireFuture {
      semaphore: self.clone(),
      dropped_flags: Default::default(),
    }
  }

  pub fn add_permits(&self, amount: usize) {
    let wakers = {
      let mut wakers = Vec::with_capacity(amount);
      let mut state = self.state.borrow_mut();
      state.max_permits += amount;

      let mut i = 0;
      while i < amount {
        match state.wakers.pop_front() {
          Some(waker) => {
            if !waker.is_future_dropped.is_raised() {
              wakers.push(waker);
              i += 1;
            }
          }
          None => break,
        }
      }
      wakers
    };
    for waker in wakers {
      waker.inner.wake();
    }
  }

  pub fn remove_permits(&self, amount: usize) {
    let mut state = self.state.borrow_mut();
    state.max_permits -= std::cmp::min(state.max_permits, amount);
  }

  pub fn max_permits(&self) -> usize {
    self.state.borrow().max_permits
  }

  /// The number of executing permits.
  ///
  /// This may be larger than the maximum number of permits
  /// if permits were removed while more than the current
  /// max were acquired.
  pub fn acquired_permits(&self) -> usize {
    self.state.borrow().acquired_permits
  }

  pub fn close(&self) {
    let wakers = {
      let mut state = self.state.borrow_mut();
      state.closed = true;
      std::mem::take(&mut state.wakers)
    };
    for waker in wakers {
      waker.inner.wake();
    }
  }

  pub fn closed(&self) -> bool {
    self.state.borrow().closed
  }

  fn release(&self) {
    let maybe_waker = {
      let mut state = self.state.borrow_mut();

      state.acquired_permits -= 1;
      if state.acquired_permits < state.max_permits {
        let mut found_waker = None;
        while let Some(waker) = state.wakers.pop_front() {
          if !waker.is_future_dropped.is_raised() {
            found_waker = Some(waker);
            break;
          }
        }
        found_waker
      } else {
        None
      }
    };

    if let Some(waker) = maybe_waker {
      waker.inner.wake();
    }
  }
}

struct AcquireFuture {
  semaphore: Rc<Semaphore>,
  dropped_flags: RefCell<Vec<Rc<Flag>>>,
}

impl Drop for AcquireFuture {
  fn drop(&mut self) {
    let mut flags = self.dropped_flags.borrow_mut();
    for flag in flags.drain(..) {
      flag.raise();
    }
  }
}

impl Future for AcquireFuture {
  type Output = Result<SemaphorePermit, ()>;

  fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
    let mut state = self.semaphore.state.borrow_mut();

    if state.closed {
      Poll::Ready(Err(()))
    } else if state.acquired_permits < state.max_permits {
      state.acquired_permits += 1;
      Poll::Ready(Ok(SemaphorePermit(self.semaphore.clone())))
    } else {
      let is_future_dropped = Rc::new(Flag::default());
      let waker = SemaphoreStateWaker {
        inner: cx.waker().clone(),
        is_future_dropped: is_future_dropped.clone(),
      };
      state.wakers.push_back(waker);
      self.dropped_flags.borrow_mut().push(is_future_dropped);
      Poll::Pending
    }
  }
}

pub struct Notify(Rc<Semaphore>);

impl Default for Notify {
    fn default() -> Self {
    Self(Rc::new(Semaphore::new(0)))
    }
}

impl Notify {
  pub fn notify_one(&self) {
    self.0.release();
  }

  pub fn notify_all(&self) {
    let mut state = self.0.state.borrow_mut();
    let wakers = std::mem::take(&mut state.wakers);
    for waker in wakers {
      waker.inner.wake();
    }
  }

}

#[cfg(test)]
mod test {
  use std::time::Duration;

  use super::*;

  #[tokio::test]
  async fn semaphore() {
    let semaphore = Rc::new(Semaphore::new(2));
    let permit1 = semaphore.acquire().await;
    let permit2 = semaphore.acquire().await;
    let mut hit_timeout = false;
    tokio::select! {
      _ = semaphore.acquire() => {}
      _ = tokio::time::sleep(Duration::from_millis(20)) => {
        hit_timeout = true;
      }
    }
    assert!(hit_timeout);

    // ensure the previous future's pending permit is removed
    // from the list of wakers, otherwise this will hang forever
    {
      let notify = Rc::new(Notify::new());
      let result = crate::spawn({
        let notify = notify.clone();
        let semaphore = semaphore.clone();
        async move {
          notify.notify_one();
          semaphore.acquire().await.unwrap();
        }
      });

      notify.notified().await;
      drop(permit2);
      let permit3 = result.await;
      drop(permit1);
      drop(permit3);
    }

    // test adding permits
    {
      let permit1 = semaphore.acquire().await;
      let permit2 = semaphore.acquire().await;
      semaphore.add_permits(1);
      let permit3 = semaphore.acquire().await;

      let notify = Rc::new(Notify::new());
      let result = crate::spawn({
        let notify = notify.clone();
        let semaphore = semaphore.clone();
        async move {
          notify.notify_one();
          let permit1 = semaphore.acquire().await.unwrap();
          let permit2 = semaphore.acquire().await.unwrap();
          drop(permit2);
          drop(permit1);
        }
      });
      notify.notified().await;
      semaphore.add_permits(2);
      result.await.unwrap();
      drop(permit1);
      drop(permit2);
      drop(permit3);
    }

    // test removing permits
    {
      semaphore.remove_permits(semaphore.max_permits() - 1);
      let permit = semaphore.acquire().await;
      let mut hit_timeout = false;
      tokio::select! {
        _ = semaphore.acquire() => {}
        _ = tokio::time::sleep(Duration::from_millis(20)) => {
          hit_timeout = true;
        }
      }
      assert!(hit_timeout);
      drop(permit);
      semaphore.add_permits(2);
      let permit1 = semaphore.acquire().await;
      let permit2 = semaphore.acquire().await;
      let permit3 = semaphore.acquire().await;
      semaphore.remove_permits(1);
      let notify = Rc::new(Notify::new());
      let notify_complete = Rc::new(RefCell::new(false));
      let result = crate::spawn({
        let notify = notify.clone();
        let notify_complete = notify_complete.clone();
        let semaphore = semaphore.clone();
        async move {
          notify.notify_one();
          semaphore.acquire().await.unwrap();
          *notify_complete.borrow_mut() = true;
        }
      });
      notify.notified().await;
      drop(permit3);

      tokio::time::sleep(Duration::from_millis(20)).await;
      assert_eq!(*notify_complete.borrow(), false);

      drop(permit1);
      result.await.unwrap();
      assert_eq!(*notify_complete.borrow(), true);
      drop(permit2);
    }
  }
}

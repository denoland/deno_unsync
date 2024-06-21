// Copyright 2018-2024 the Deno authors. MIT license.

use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

impl<T: ?Sized> LocalFutureExt for T where T: Future {}

pub trait LocalFutureExt: std::future::Future {
  fn shared_local(self) -> SharedLocal<Self::Output>
  where
    Self: Sized + 'static,
    Self::Output: Clone,
  {
    SharedLocal::new(Box::pin(self))
  }
}

enum FutureOrResult<TOutput: Clone> {
  Future(Pin<Box<dyn Future<Output = TOutput>>>),
  Result(TOutput),
}

impl<TOutput: Clone + std::fmt::Debug> std::fmt::Debug
  for FutureOrResult<TOutput>
{
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      Self::Future(_) => f.debug_tuple("Future").field(&"<pending>").finish(),
      Self::Result(arg0) => f.debug_tuple("Result").field(arg0).finish(),
    }
  }
}

/// A !Send-friendly future whose result can be awaited multiple times.
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct SharedLocal<TOutput: Clone>(Rc<RefCell<FutureOrResult<TOutput>>>);

impl<TOutput: Clone> Clone for SharedLocal<TOutput> {
  fn clone(&self) -> Self {
    Self(self.0.clone())
  }
}

impl<TOutput: Clone + std::fmt::Debug> std::fmt::Debug
  for SharedLocal<TOutput>
{
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    f.debug_tuple("SharedLocal").field(&self.0).finish()
  }
}

impl<TOutput: Clone> SharedLocal<TOutput> {
  pub fn new(future: Pin<Box<dyn Future<Output = TOutput>>>) -> Self {
    SharedLocal(Rc::new(RefCell::new(FutureOrResult::Future(future))))
  }
}

impl<TOutput: Clone> std::future::Future for SharedLocal<TOutput> {
  type Output = TOutput;

  fn poll(
    self: std::pin::Pin<&mut Self>,
    cx: &mut std::task::Context<'_>,
  ) -> std::task::Poll<Self::Output> {
    use std::task::Poll;

    let mut inner = self.0.borrow_mut();
    match &mut *inner {
      FutureOrResult::Future(fut) => match fut.as_mut().poll(cx) {
        Poll::Ready(result) => {
          *inner = FutureOrResult::Result(result.clone());
          Poll::Ready(result)
        }
        Poll::Pending => Poll::Pending,
      },
      FutureOrResult::Result(result) => Poll::Ready(result.clone()),
    }
  }
}

#[cfg(test)]
mod test {
  use super::LocalFutureExt;

  #[tokio::test]
  async fn test_shared_local_future() {
    let shared = super::SharedLocal::new(Box::pin(async { 42 }));
    assert_eq!(shared.clone().await, 42);
    assert_eq!(shared.await, 42);
  }

  #[tokio::test]
  async fn test_shared_local() {
    let shared = async { 42 }.shared_local();
    assert_eq!(shared.clone().await, 42);
    assert_eq!(shared.await, 42);
  }
}

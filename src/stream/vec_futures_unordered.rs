use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use futures_core::Stream;

/// A ![`Sync`] and ![`Sync`] version of `futures::stream::FuturesUnordered`
/// that is backed by a vector.
/// 
/// This is useful if you know the number of the futures you
/// want to collect ahead of time.
pub struct VecFuturesUnordered<F: std::future::Future>(Vec<F>);

impl<F: std::future::Future> VecFuturesUnordered<F> {
  pub fn with_capacity(capacity: usize) -> Self {
    Self(Vec::with_capacity(capacity))
  }

  pub fn is_empty(&self) -> bool {
    self.0.is_empty()
  }

  pub fn len(&self) -> usize {
    self.0.len()
  }

  pub fn push(&mut self, future: F) {
    self.0.push(future);
  }
}

impl<F> Stream for VecFuturesUnordered<F>
where
  F: std::future::Future + Unpin,
{
  type Item = F::Output;

  fn poll_next(
    mut self: Pin<&mut Self>,
    cx: &mut Context<'_>,
  ) -> Poll<Option<Self::Item>> {
    if self.0.is_empty() {
      return Poll::Ready(None);
    }

    for (i, future) in self.0.iter_mut().enumerate() {
      let future = Pin::new(future);
      match future.poll(cx) {
        Poll::Ready(output) => {
          self.0.swap_remove(i);
          return Poll::Ready(Some(output))
        },
        Poll::Pending => {}
      }
    }
    Poll::Pending
  }

  fn size_hint(&self) -> (usize, Option<usize>) {
    let len = self.len();
    (len, Some(len))
  }
}

#[cfg(test)]
mod test {
  use std::time::Duration;

  use futures::FutureExt;
  use futures::StreamExt;

  use super::*;

  #[tokio::test(flavor = "current_thread")]
  async fn completes_first_to_finish_time() {
    let mut futures = VecFuturesUnordered::with_capacity(3);
    futures.push(
      async {
        tokio::time::sleep(Duration::from_millis(100)).await;
        1
      }
      .boxed_local(),
    );
    futures.push(
      async {
        tokio::time::sleep(Duration::from_millis(50)).await;
        2
      }
      .boxed_local(),
    );
    futures.push(
      async {
        tokio::time::sleep(Duration::from_millis(25)).await;
        3
      }
      .boxed_local(),
    );

    let first = futures.next().await.unwrap();
    let second = futures.next().await.unwrap();
    let third = futures.next().await.unwrap();
    assert_eq!(first, 3);
    assert_eq!(second, 2);
    assert_eq!(third, 1);
  }

  #[tokio::test(flavor = "current_thread")]
  async fn completes_first_to_finish_polls() {
    let mut futures = VecFuturesUnordered::with_capacity(3);
    futures.push(
      async {
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        1
      }
      .boxed_local(),
    );
    futures.push(
      async {
        tokio::task::yield_now().await;
        tokio::task::yield_now().await;
        2
      }
      .boxed_local(),
    );
    futures.push(
      async {
        tokio::task::yield_now().await;
        3
      }
      .boxed_local(),
    );
    assert_eq!(futures.len(), 3);

    let first = futures.next().await.unwrap();
    assert_eq!(futures.len(), 2);
    let second = futures.next().await.unwrap();
    assert_eq!(futures.len(), 1);
    assert!(!futures.is_empty());
    let third = futures.next().await.unwrap();
    assert_eq!(futures.len(), 0);
    assert!(futures.is_empty());
    assert_eq!(first, 3);
    assert_eq!(second, 2);
    assert_eq!(third, 1);
  }
}

use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use futures_core::Stream;

struct OneDirectionalLinkedListNode<F> {
  future: F,
  next: Option<Box<OneDirectionalLinkedListNode<F>>>,
}

/// A ![`Sync`] and ![`Sync`] version of `futures::stream::FuturesUnordered`.
pub struct FuturesUnordered<F> {
  inner: Option<Box<OneDirectionalLinkedListNode<F>>>,
}

impl<F> FuturesUnordered<F> {
  pub fn new() -> Self {
    Self { inner: None }
  }

  pub fn is_empty(&self) -> bool {
    self.inner.is_none()
  }

  pub fn push(&mut self, future: F) {
    let past = self.inner.take();
    self.inner = Some(Box::new(OneDirectionalLinkedListNode {
      future,
      next: past,
    }));
  }
}

impl<F> Stream for FuturesUnordered<F>
where
  F: std::future::Future + Unpin,
{
  type Item = F::Output;

  fn poll_next(
    mut self: Pin<&mut Self>,
    cx: &mut Context<'_>,
  ) -> Poll<Option<Self::Item>> {
    if let Some(first) = self.inner.as_mut() {
      let future = Pin::new(&mut first.future);
      match future.poll(cx) {
        Poll::Ready(output) => {
          self.inner = first.next.take();
          return Poll::Ready(Some(output));
        }
        Poll::Pending => {
          let mut current = first;
          while let Some(mut next) = current.next.take() {
            let future = Pin::new(&mut next.future);
            match future.poll(cx) {
              Poll::Ready(output) => {
                current.next = next.next.take();
                return Poll::Ready(Some(output));
              }
              Poll::Pending => {
                current.next = Some(next);
                current = current.next.as_mut().unwrap();
              }
            }
          }
        }
      }

      Poll::Pending
    } else {
      return Poll::Ready(None);
    }
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
    let mut futures = FuturesUnordered::new();
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
    let mut futures = FuturesUnordered::new();
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

    let first = futures.next().await.unwrap();
    let second = futures.next().await.unwrap();
    let third = futures.next().await.unwrap();
    assert_eq!(first, 3);
    assert_eq!(second, 2);
    assert_eq!(third, 1);
  }
}

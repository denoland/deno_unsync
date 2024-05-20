use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use futures_core::Stream;

struct OneDirectionalLinkedListNode<F> {
  future: F,
  next: Option<Box<OneDirectionalLinkedListNode<F>>>,
}

/// A ![`Sync`] and ![`Sync`] version of `futures::stream::FuturesUnordered`.
pub struct LocalFuturesUnordered<F> {
  inner: Option<Box<OneDirectionalLinkedListNode<F>>>,
}

impl<F> LocalFuturesUnordered<F> {
  pub fn new() -> Self {
      Self {
          inner: None,
      }
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

impl<F> Stream for LocalFuturesUnordered<F>
where
  F: std::future::Future + Unpin,
{
  type Item = F::Output;

  fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
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
    }

    if self.inner.is_none() {
        Poll::Ready(None)
    } else {
        Poll::Pending
    }
  }
}

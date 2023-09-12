use std::cell::RefCell;
use std::collections::LinkedList;
use std::future::Future;
use std::rc::Rc;
use std::task::Waker;

use crate::Flag;

#[derive(Debug, Default)]
struct TaskQueueTaskWaker {
  is_ready: Flag,
  waker: RefCell<Option<Waker>>,
  dropped_before_waker: Flag,
}

#[derive(Debug, Default)]
struct TaskQueueTasks {
  is_running: bool,
  wakers: LinkedList<Rc<TaskQueueTaskWaker>>,
}

/// A queue that executes tasks sequentially one after the other
/// ensuring order and that no task runs at the same time as another.
#[derive(Debug, Default)]
pub struct TaskQueue {
  tasks: RefCell<TaskQueueTasks>,
}

impl TaskQueue {
  /// Acquires a permit where the tasks are executed one at a time
  /// and in the order that they were acquired.
  pub fn acquire(&self) -> TaskQueuePermitAcquireFuture {
    TaskQueuePermitAcquireFuture::new(self)
  }

  /// Alternate API that acquires a permit internally
  /// for the duration of the future.
  pub async fn run<R>(&self, future: impl Future<Output = R>) -> R {
    let _permit = self.acquire().await;
    future.await
  }
}

/// A permit that when dropped will allow another task to proceed.
pub struct TaskQueuePermit<'a>(&'a TaskQueue);

impl<'a> Drop for TaskQueuePermit<'a> {
  fn drop(&mut self) {
    let next_item = {
      let mut tasks = self.0.tasks.borrow_mut();
      let next_item = tasks.wakers.pop_front();
      tasks.is_running = next_item.is_some();
      next_item
    };
    if let Some(next_item) = next_item {
      next_item.is_ready.raise();
      if let Some(waker) = next_item.waker.borrow_mut().take() {
        waker.wake();
      } else {
        next_item.dropped_before_waker.raise();
      }
    }
  }
}

pub struct TaskQueuePermitAcquireFuture<'a> {
  task_queue: &'a TaskQueue,
  initialized: RefCell<bool>,
  waker: Rc<TaskQueueTaskWaker>,
}

impl<'a> TaskQueuePermitAcquireFuture<'a> {
  pub fn new(task_queue: &'a TaskQueue) -> Self {
    Self {
      task_queue,
      initialized: Default::default(),
      waker: Default::default(),
    }
  }
}

impl<'a> Future for TaskQueuePermitAcquireFuture<'a> {
  type Output = TaskQueuePermit<'a>;

  fn poll(
    self: std::pin::Pin<&mut Self>,
    cx: &mut std::task::Context<'_>,
  ) -> std::task::Poll<Self::Output> {
    {
      let mut stored_waker = self.waker.waker.borrow_mut();
      // update with the latest waker if it's different or not set
      if stored_waker
        .as_ref()
        .map(|w| !w.will_wake(cx.waker()))
        .unwrap_or(true)
      {
        *stored_waker = Some(cx.waker().clone());

        if self.waker.dropped_before_waker.is_raised() {
          return std::task::Poll::Ready(TaskQueuePermit(self.task_queue));
        }
      }
    }

    // ensure this is initialized
    let initialized = {
      let mut value = self.initialized.borrow_mut();
      let current_value = *value;
      *value = true;
      current_value
    };
    if !initialized {
      let mut tasks = self.task_queue.tasks.borrow_mut();
      if !tasks.is_running {
        tasks.is_running = true;
        return std::task::Poll::Ready(TaskQueuePermit(self.task_queue));
      }
      tasks.wakers.push_back(self.waker.clone());
      return std::task::Poll::Pending;
    }

    // check if we're ready to run
    if self.waker.is_ready.is_raised() {
      std::task::Poll::Ready(TaskQueuePermit(self.task_queue))
    } else {
      std::task::Poll::Pending
    }
  }
}

#[cfg(test)]
mod tests {
  use std::sync::Arc;
  use std::sync::Mutex;

  use crate::JoinSet;

  use super::*;

  #[tokio::test]
  async fn task_queue_runs_one_after_other() {
    let task_queue = Rc::new(TaskQueue::default());
    let mut set = JoinSet::default();
    let data = Arc::new(Mutex::new(0));
    for i in 0..100 {
      let data = data.clone();
      let task_queue = task_queue.clone();
      set.spawn(async move {
        task_queue
          .run(async move {
            crate::spawn_blocking(move || {
              let mut data = data.lock().unwrap();
              if *data != i {
                panic!("Value was not equal.");
              }
              *data = i + 1;
            })
            .await
            .unwrap();
          })
          .await
      });
    }
    while let Some(res) = set.join_next().await {
      assert!(res.is_ok());
    }
  }

  #[tokio::test]
  async fn tasks_run_in_sequence() {
    let task_queue = TaskQueue::default();
    let data = RefCell::new(0);

    let first = task_queue.run(async {
      *data.borrow_mut() = 1;
    });
    let second = task_queue.run(async {
      assert_eq!(*data.borrow(), 1);
      *data.borrow_mut() = 2;
    });
    let _ = tokio::join!(first, second);

    assert_eq!(*data.borrow(), 2);
  }

  #[tokio::test]
  async fn dropped_before_waker() {
    let task_queue = Rc::new(TaskQueue::default());

    // Acquire a permit but do not release it immediately.
    let permit = task_queue.acquire();

    // This task tries to acquire another permit, but will be blocked by the first permit.
    let delayed_task = crate::spawn({
      let task_queue = task_queue.clone();
      async move {
        task_queue.acquire().await;
        true
      }
    });

    // ensure the task gets a chance to be scheduled and blocked
    tokio::task::yield_now().await;

    // Now, drop the first permit. This will trigger the mechanism around `dropped_before_waker`.
    drop(permit);

    assert!(delayed_task.await.unwrap());
  }
}

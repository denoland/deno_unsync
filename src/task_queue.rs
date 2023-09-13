use std::cell::RefCell;
use std::cell::UnsafeCell;
use std::collections::LinkedList;
use std::future::Future;
use std::rc::Rc;
use std::task::Waker;

use crate::Flag;

#[derive(Debug, Default)]
struct TaskQueueTaskWaker {
  is_ready: Flag,
  is_future_dropped: Flag,
  waker: FastOptionCell<Waker>,
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
  pub fn acquire(self: &Rc<Self>) -> TaskQueuePermitAcquireFuture {
    TaskQueuePermitAcquireFuture::new(self.clone())
  }

  /// Alternate API that acquires a permit internally
  /// for the duration of the future.
  pub fn run<R>(self: &Rc<Self>, future: impl Future<Output = R>) -> impl Future<Output = R> {
    let acquire_future = self.acquire();
    async move {
      let permit = acquire_future.await;
      let result = future.await;
      drop(permit); // explicit for clarity
      result
    }
  }
}

/// A permit that when dropped will allow another task to proceed.
pub struct TaskQueuePermit(Rc<TaskQueue>);

impl Drop for TaskQueuePermit {
  fn drop(&mut self) {
    let next_item = {
      let mut tasks = self.0.tasks.borrow_mut();
      let next_waker = tasks.wakers.pop_front();
      tasks.is_running = next_waker.is_some();
      next_waker
    };
    if let Some(next_waker) = next_item {
      next_waker.is_ready.raise();
      if let Some(waker) = next_waker.waker.take() {
        waker.wake();
      }
    }
  }
}

pub struct TaskQueuePermitAcquireFuture {
  task_queue: FastOptionCell<Rc<TaskQueue>>,
  waker: Rc<TaskQueueTaskWaker>,
}

impl Drop for TaskQueuePermitAcquireFuture {
  fn drop(&mut self) {
    if let Some(task_queue) = self.task_queue.take() {
    self.waker.is_future_dropped.raise();

    if self.waker.is_ready.is_raised() {
      let mut tasks = task_queue.tasks.borrow_mut();

      // clear out any wakers for futures that were drpped
      while let Some(front_waker) = tasks.wakers.front() {
        if front_waker.is_future_dropped.is_raised() {
          tasks.wakers.pop_front();
        } else {
          break;
        }
      }

      // wake up te next waker
      tasks.is_running = tasks.wakers.front().is_some();
      if let Some(front_waker) = tasks.wakers.pop_front() {
        front_waker.is_ready.raise();
        if let Some(waker) = front_waker.waker.take() {
          waker.wake();
        }
      }
    }
  }
  }
}

impl TaskQueuePermitAcquireFuture {
  pub fn new(task_queue: Rc<TaskQueue>) -> Self {
    let waker = Rc::new(TaskQueueTaskWaker::default());
    let mut tasks = task_queue.tasks.borrow_mut();
    if !tasks.is_running {
      tasks.is_running = true;
      waker.is_ready.raise();
    } else {
      tasks.wakers.push_back(waker.clone());
    }
    drop(tasks);
    Self {
      task_queue: FastOptionCell::new(task_queue),
      waker,
    }
  }
}

impl Future for TaskQueuePermitAcquireFuture {
  type Output = TaskQueuePermit;

  fn poll(
    self: std::pin::Pin<&mut Self>,
    cx: &mut std::task::Context<'_>,
  ) -> std::task::Poll<Self::Output> {
    // check if we're ready to run
    if self.waker.is_ready.is_raised() {
      // we're done, move the task queue out
      std::task::Poll::Ready(TaskQueuePermit(self.task_queue.take().unwrap()))
    } else {
      // store the waker for next time
      self.waker.waker.replace_if_with(
        |w| w.as_ref().map(|w| !w.will_wake(cx.waker())).unwrap_or(true),
        || cx.waker().clone(),
      );
      std::task::Poll::Pending
    }
  }
}

#[derive(Debug)]
struct FastOptionCell<T>(UnsafeCell<Option<T>>);

impl<T> Default for FastOptionCell<T> {
    fn default() -> Self {
      Self(Default::default())
    }
}

impl<T> FastOptionCell<T> {
  pub fn new(value: T) -> Self {
    Self(UnsafeCell::new(Some(value)))
  }

  pub fn take(&self) -> Option<T> {
    unsafe {
      let value = &mut *self.0.get();
      value.take()
    }
  }

  pub fn replace_if_with(
    &self,
    if_condition: impl FnOnce(&Option<T>) -> bool,
    replace_with: impl FnOnce() -> T,
  ) {
    // update with the latest waker if it's different or not set
    unsafe {
      let value = &mut *self.0.get();
      if if_condition(value) {
        *value = Some(replace_with());
      }
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
      let acquire = task_queue.acquire();
      set.spawn(async move {
        let permit = acquire.await;
        crate::spawn_blocking(move || {
          let mut data = data.lock().unwrap();
          assert_eq!(i, *data);
          *data = i + 1;
        })
        .await
        .unwrap();
        drop(permit);
        drop(task_queue);
      });
    }
    while let Some(res) = set.join_next().await {
      assert!(res.is_ok());
    }
  }

  #[tokio::test]
  async fn tasks_run_in_sequence() {
    let task_queue = Rc::new(TaskQueue::default());
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
  async fn future_dropped_before_poll() {
    let task_queue = Rc::new(TaskQueue::default());

    // acquire a future, but do not await it
    let future = task_queue.acquire();

    // this task tries to acquire another permit, but will be blocked by the first permit.
    let flag = Rc::new(Flag::default());
    let delayed_task = crate::spawn({
      let task_queue = task_queue.clone();
      let flag = flag.clone();
      async move {
        flag.raise();
        task_queue.acquire().await;
        true
      }
    });

    // ensure the task gets a chance to be scheduled and blocked
    tokio::task::yield_now().await;
    assert!(flag.is_raised());

    // now, drop the first future
    drop(future);

    assert!(delayed_task.await.unwrap());
  }

  #[tokio::test]
  async fn many_future_dropped_before_poll() {
    let task_queue = Rc::new(TaskQueue::default());

    // acquire a future, but do not await it
    let mut futures = Vec::new();
    for _ in 0..5 {
      futures.push(task_queue.acquire());
    }

    // this task tries to acquire another permit, but will be blocked by the first permit.
    let flag = Rc::new(Flag::default());
    let delayed_task = crate::spawn({
      let task_queue = task_queue.clone();
      let flag = flag.clone();
      async move {
        flag.raise();
        task_queue.acquire().await;
        true
      }
    });

    // ensure the task gets a chance to be scheduled and blocked
    tokio::task::yield_now().await;
    assert!(flag.is_raised());

    // now, drop the futures
    drop(futures);

    assert!(delayed_task.await.unwrap());
  }
}

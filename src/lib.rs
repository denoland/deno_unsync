mod flag;
mod joinset;
mod split;
mod task;
mod task_queue;
mod waker;

pub use flag::Flag;
pub use joinset::JoinSet;
pub use split::split_io;
pub use split::IOReadHalf;
pub use split::IOWriteHalf;
pub use task::spawn;
pub use task::spawn_blocking;
pub use task::JoinHandle;
pub use task::MaskFutureAsSend;
pub use task_queue::TaskQueue;
pub use task_queue::TaskQueuePermit;
pub use task_queue::TaskQueuePermitAcquireFuture;
pub use waker::UnsyncWaker;

/// Marker for items that are ![`Send`].
#[derive(Copy, Clone, Default, Eq, PartialEq, PartialOrd, Ord, Debug, Hash)]
pub struct UnsendMarker(
  std::marker::PhantomData<std::sync::MutexGuard<'static, ()>>,
);

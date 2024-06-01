use criterion::{criterion_group, criterion_main, Criterion};
use deno_unsync::stream::VecFuturesUnordered;
use futures::{stream::FuturesUnordered, FutureExt, StreamExt as _};

// const LEN: usize = 100_000;
// const SUM: usize = 4999950000;
const LEN: usize = 100;
const SUM: usize = 4950;

async fn send_many_tasks_unsync() {
  let mut futures = VecFuturesUnordered::with_capacity(LEN);
  for i in 0..LEN {
    futures.push(async move {
      if i % 2 == 0 {
        tokio::task::yield_now().await;
      }
      tokio::task::yield_now().await;
      i
    }.boxed_local());
  }

  let mut sum = 0;
  while let Some(value) = futures.next().await {
    sum += value;
  }
  assert_eq!(sum, SUM);
}

async fn send_many_tasks_sync() {
  let mut futures = FuturesUnordered::new();
  for i in 0..LEN {
    futures.push(async move {
      if i % 2 == 0 {
        tokio::task::yield_now().await;
      }
      tokio::task::yield_now().await;
      i
    }.boxed_local());
  }

  let mut sum = 0;
  while let Some(value) = futures.next().await {
    sum += value;
  }
  assert_eq!(sum, SUM);
}

fn criterion_benchmark(c: &mut Criterion) {
  c.bench_function("bench unsync", |b| {
    b.iter(|| {
      let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
      rt.block_on(async {
        send_many_tasks_unsync().await;
      });
    })
  });
  c.bench_function("bench sync", |b| {
    b.iter(|| {
      let rt = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
      rt.block_on(async {
        send_many_tasks_sync().await;
      });
    })
  });
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
use std::{
    sync::Barrier,
    thread,
    time::{Duration, Instant},
};

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

trait Testee: Send + Sync {
    type State;

    fn make_state(&self, thread_no: u32) -> Self::State;
    fn exec(&self, state: &mut Self::State);
}

fn run(thread_count: u32, iter_count: u64, testee: &impl Testee) -> Duration {
    let start_barrier = Barrier::new(1 + thread_count as usize);
    let end_barrier = Barrier::new(1 + thread_count as usize);

    thread::scope(|scope| {
        let mut handles = Vec::new();

        for thread_no in 0..thread_count {
            let start_barrier = &start_barrier;
            let end_barrier = &end_barrier;

            let handle = scope.spawn(move || {
                let mut state = testee.make_state(thread_no);

                start_barrier.wait();

                for _ in 0..iter_count {
                    testee.exec(black_box(&mut state));
                }

                end_barrier.wait();
            });

            handles.push(handle);
        }

        start_barrier.wait();
        let start = Instant::now();
        end_barrier.wait();
        let elapsed = start.elapsed();

        for handle in handles {
            handle.join().unwrap();
        }

        elapsed
    })
}

#[derive(Copy, Clone)]
#[repr(align(128))] // avoid false sharing (relevant for sharded-slab)
struct Value(u64);

fn only_read(c: &mut Criterion) {
    let mut group = c.benchmark_group("only_read");

    // Cache an immutable setup to avoid re-creating the testees for each benchmark.
    let mut idr_repin_testee = None;
    let mut idr_pin_once_testee = None;
    let mut sharded_slab_testee = None;

    for contention in parallelism() {
        group.bench_with_input(
            BenchmarkId::new("idr-repin", contention),
            &contention,
            |b, _| {
                let testee = idr_repin_testee.get_or_insert_with(|| IdrTestee::new(false));
                b.iter_custom(|iter_count| run(contention, iter_count, testee));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("idr-pin-once", contention),
            &contention,
            |b, _| {
                let testee = idr_pin_once_testee.get_or_insert_with(|| IdrTestee::new(true));
                b.iter_custom(|iter_count| run(contention, iter_count, testee));
            },
        );

        group.bench_with_input(
            BenchmarkId::new("sharded-slab", contention),
            &contention,
            |b, _| {
                let testee = sharded_slab_testee.get_or_insert_with(ShardedSlabTestee::new);
                b.iter_custom(|iter_count| run(contention, iter_count, testee));
            },
        );
    }
    group.finish();

    struct IdrTestee {
        idr: idr_ebr::Idr<Value>,
        key: idr_ebr::Key,
        pin_once: bool,
    }

    impl IdrTestee {
        fn new(pin_once: bool) -> Self {
            let idr = idr_ebr::Idr::new();
            let mut key = None;

            for i in 0u64..1_000 {
                let k = idr.insert(Value(i)).unwrap();

                if i == 500 {
                    assert_eq!(idr.get(k).unwrap().0, i); // sanity check
                    key = Some(k);
                }
            }

            let key = key.unwrap();
            Self { idr, key, pin_once }
        }
    }

    impl Testee for IdrTestee {
        type State = Option<scc::ebr::Guard>;

        fn make_state(&self, _thread_no: u32) -> Self::State {
            let guard = scc::ebr::Guard::new(); // warm up
            self.pin_once.then_some(guard)
        }

        fn exec(&self, _: &mut Self::State) {
            black_box(self.idr.get(self.key));
        }
    }

    struct ShardedSlabTestee {
        slab: sharded_slab::Slab<Value>,
        key: usize,
    }

    impl ShardedSlabTestee {
        fn new() -> Self {
            let slab = sharded_slab::Slab::new();
            let mut key = None;

            for i in 0u64..1_000 {
                let k = slab.insert(Value(i)).unwrap();

                if i == 500 {
                    assert_eq!(slab.get(k).unwrap().0, i); // sanity check
                    key = Some(k);
                }
            }

            let key = key.unwrap();
            Self { slab, key }
        }
    }

    impl Testee for ShardedSlabTestee {
        type State = ();

        fn make_state(&self, _thread_no: u32) -> Self::State {}

        fn exec(&self, _: &mut Self::State) {
            black_box(self.slab.get(self.key));
        }
    }
}

fn insert_remove(c: &mut Criterion) {
    let mut group = c.benchmark_group("insert_remove");

    // Cache an immutable setup to avoid re-creating the testees for each benchmark.
    let mut idr_testee = None;
    let mut sharded_slab_testee = None;

    for contention in parallelism() {
        group.bench_with_input(BenchmarkId::new("idr", contention), &contention, |b, _| {
            let testee = idr_testee.get_or_insert_with(IdrTestee::new);
            b.iter_custom(|iter_count| run(contention, iter_count, testee));
        });

        group.bench_with_input(
            BenchmarkId::new("sharded-slab", contention),
            &contention,
            |b, _| {
                let testee = sharded_slab_testee.get_or_insert_with(ShardedSlabTestee::new);
                b.iter_custom(|iter_count| run(contention, iter_count, testee));
            },
        );
    }
    group.finish();

    struct IdrTestee {
        idr: idr_ebr::Idr<Value>,
    }

    impl IdrTestee {
        fn new() -> Self {
            let idr = idr_ebr::Idr::new();

            let keys = (0u64..100_000)
                .map(|i| (idr.insert(Value(i)).unwrap(), i))
                .filter(|(_, i)| i % 2 == 0)
                .map(|(key, _)| key)
                .collect::<Vec<_>>();

            // Remove every other entry.
            for key in keys {
                idr.remove(key);
                assert!(!idr.contains(key)); // sanity check
            }

            Self { idr }
        }
    }

    impl Testee for IdrTestee {
        type State = Value;

        fn make_state(&self, thread_no: u32) -> Self::State {
            Value(u64::from(thread_no))
        }

        fn exec(&self, state: &mut Self::State) {
            let key = self.idr.insert(*state).unwrap();
            self.idr.remove(key);
        }
    }

    struct ShardedSlabTestee {
        slab: sharded_slab::Slab<Value>,
    }

    impl ShardedSlabTestee {
        fn new() -> Self {
            let slab = sharded_slab::Slab::new();

            let keys = (0u64..100_000)
                .map(|i| (slab.insert(Value(i)).unwrap(), i))
                .filter(|(_, i)| i % 2 == 0)
                .map(|(key, _)| key)
                .collect::<Vec<_>>();

            // Remove every other entry.
            for key in keys {
                slab.remove(key);
                assert!(!slab.contains(key)); // sanity check
            }

            Self { slab }
        }
    }

    impl Testee for ShardedSlabTestee {
        type State = Value;

        fn make_state(&self, thread_no: u32) -> Self::State {
            Value(u64::from(thread_no))
        }

        fn exec(&self, state: &mut Self::State) {
            let key = self.slab.insert(*state).unwrap();
            self.slab.remove(key);
        }
    }
}

fn parallelism() -> Vec<u32> {
    let max = thread::available_parallelism().unwrap().get() as u32;
    (1..=max).collect()
}

criterion_group!(cases, only_read, insert_remove);
criterion_main!(cases);

#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

#![cfg(loom)]

use std::sync::Arc;

use loom::{
    sync::{Condvar, Mutex},
    thread,
};

use idr_ebr::{Config, Guard, Idr, Key};

// === Helpers ===

fn run_model<F>(f: F)
where
    F: Fn() + Sync + Send + 'static,
{
    use std::{
        io::Write,
        sync::atomic::{AtomicU32, Ordering},
    };

    let iters = Arc::new(AtomicU32::new(0));
    let iters1 = iters.clone();

    loom::model(move || {
        iters.fetch_add(1, Ordering::Relaxed);
        f();
    });

    let iters = iters1.load(Ordering::Relaxed);
    #[allow(clippy::explicit_write)] // print even when stdout is captured
    write!(std::io::stdout(), "[{iters} iters] ").unwrap();
}

macro_rules! show {
    ($val:expr) => {{
        let line = line!();
        let expr = stringify!($val);

        tracing::debug!("> {}: {}", line, expr);
        let res = $val;
        tracing::debug!("< {}: {} = {:?}", line, expr, res);
        res
    }};
}

// === Cases ===

struct TinyConfig;

impl Config for TinyConfig {
    const INITIAL_PAGE_SIZE: u32 = 2;
    const RESERVED_BITS: u32 = 5;
}

struct TinierConfig;

impl crate::Config for TinierConfig {
    const INITIAL_PAGE_SIZE: u32 = 2;
    const MAX_PAGES: u32 = 1;
    const RESERVED_BITS: u32 = 32;
}

// Concurrent `VacantEntry::insert()` and `get()` on the same entry.
#[test]
fn vacant_entry() {
    run_model(|| {
        let idr = Arc::new(Idr::default());
        let entry = idr.vacant_entry().unwrap();
        let key = entry.key();

        let idr1 = idr.clone();
        let t1 = thread::spawn(move || {
            let guard = Guard::new();
            let entry = show!(idr1.get(key, &guard));
            assert!(entry.is_none() || entry.unwrap() == "foo");
        });

        show!(entry.insert("foo"));
        t1.join().unwrap();

        let guard = Guard::new();
        assert_eq!(idr.get(key, &guard).unwrap(), "foo");
    });
}

// Concurrent `VacantEntry::insert()` and `get()` on the same entry.
#[test]
fn vacant_entry_2() {
    run_model(|| {
        let idr = Arc::new(Idr::default());
        let entry = idr.vacant_entry().unwrap();
        let key = entry.key();

        let idr1 = idr.clone();
        let idr2 = idr.clone();
        let t1 = thread::spawn(move || {
            let guard = Guard::new();
            let entry = show!(idr1.get(key, &guard));
            assert!(entry.is_none() || entry.unwrap() == "foo");
        });

        show!(entry.insert("foo"));

        let t2 = thread::spawn(move || {
            let guard = Guard::new();
            let entry = show!(idr2.get(key, &guard));
            assert_eq!(entry.unwrap(), "foo");
        });

        t1.join().unwrap();
        t2.join().unwrap();

        let guard = Guard::new();
        assert_eq!(idr.get(key, &guard).unwrap(), "foo");
    });
}

// Concurrent `VacantEntry::insert()` and `remove()` on the same entry.
#[test]
fn vacant_entry_remove() {
    run_model(|| {
        let idr = Arc::new(Idr::default());
        let entry = idr.vacant_entry().unwrap();
        let key = entry.key();

        let idr1 = idr.clone();
        let t1 = thread::spawn(move || {
            let removed = show!(idr1.remove(key));
            assert!(!removed);
        });

        t1.join().unwrap();

        entry.insert("foo");

        let guard = Guard::new();
        assert_eq!(idr.get(key, &guard).unwrap(), "foo");
    });
}

// A thread is inserting into a full IDR.
#[test]
fn insert_full() {
    run_model(|| {
        let idr = Arc::new(Idr::<_, TinierConfig>::new());

        let key1 = idr.insert(1).unwrap();
        let key2 = idr.insert(2).unwrap();

        assert_eq!(idr.get(key1, &Guard::new()).unwrap(), 1);
        assert_eq!(idr.get(key2, &Guard::new()).unwrap(), 2);

        let idr1 = idr.clone();
        let t1 = thread::spawn(move || show!(idr1.remove(key1)));

        let idr2 = idr.clone();
        let t2 = thread::spawn(move || show!(idr2.remove(key2)));

        let key3 = loop {
            if let Some(key) = show!(idr.insert(3)) {
                break key;
            }
            thread::yield_now();
        };

        let r1 = t1.join().unwrap();
        let r2 = t2.join().unwrap();

        let guard = Guard::new();
        assert!(r1 && r2, "both threads removed entries");
        assert!(idr.get(key1, &guard).is_none());
        assert!(idr.get(key2, &guard).is_none());
        assert_eq!(idr.get(key3, &guard).unwrap(), 3);
    });
}

// Threads insert different entries.
#[test]
fn concurrent_insert() {
    run_model(|| {
        let idr = Arc::new(Idr::<_, TinyConfig>::new());

        let idr1 = idr.clone();
        let t1 = thread::spawn(move || {
            show!(idr1.insert(1)).unwrap();
            show!(idr1.insert(2)).unwrap();
            show!(idr1.insert(3)).unwrap();
            show!(idr1.insert(4)).unwrap();
        });

        let idr2 = idr.clone();
        let t2 = thread::spawn(move || {
            show!(idr2.insert(5)).unwrap();
            show!(idr2.insert(6)).unwrap();
            show!(idr2.insert(7)).unwrap();
            show!(idr2.insert(8)).unwrap();
        });

        t1.join().unwrap();
        t2.join().unwrap();
    });
}

// Threads insert and remove different entries.
#[test]
fn concurrent_insert_remove() {
    run_model(|| {
        let idr = Arc::new(Idr::default());

        let idr1 = idr.clone();
        let t1 = thread::spawn(move || {
            let key = show!(idr1.insert(1)).unwrap();
            assert!(show!(idr1.remove(key)));
        });

        let idr2 = idr.clone();
        let t2 = thread::spawn(move || {
            let key = show!(idr2.insert(1)).unwrap();
            assert!(show!(idr2.remove(key)));
        });

        t1.join().unwrap();
        t2.join().unwrap();
    });
}

// Threads insert and remove multiple different entries.
#[test]
fn concurrent_insert_remove_multiple() {
    run_model(|| {
        let idr = Arc::new(Idr::default());

        let idr1 = idr.clone();
        let t1 = thread::spawn(move || {
            let guard = Guard::new();
            let key = show!(idr1.insert(1)).unwrap();
            assert_eq!(show!(idr1.get(key, &guard)).unwrap(), 1);
            assert!(show!(idr1.remove(key)));
            assert!(show!(idr1.get(key, &guard)).is_none());

            let key = show!(idr1.insert(2)).unwrap();
            assert_eq!(show!(idr1.get(key, &guard)).unwrap(), 2);
            assert!(show!(idr1.remove(key)));
            assert!(show!(idr1.get(key, &guard)).is_none());
        });

        let idr2 = idr.clone();
        let t2 = thread::spawn(move || {
            let guard = Guard::new();
            let key = show!(idr2.insert(3)).unwrap();
            assert_eq!(show!(idr2.get(key, &guard)).unwrap(), 3);
            assert!(show!(idr2.remove(key)));
            assert!(show!(idr2.get(key, &guard)).is_none());

            let key = show!(idr2.insert(4)).unwrap();
            assert_eq!(show!(idr2.get(key, &guard)).unwrap(), 4);
            assert!(show!(idr2.remove(key)));
            assert!(show!(idr2.get(key, &guard)).is_none());
        });

        t1.join().unwrap();
        t2.join().unwrap();
    });
}

// Threads remove different entries.
#[test]
fn concurrent_remove() {
    run_model(|| {
        let idr = Arc::new(Idr::default());

        let key0 = idr.insert(0).unwrap();
        assert_eq!(idr.get(key0, &Guard::new()).unwrap(), 0);
        let key1 = idr.insert(1).unwrap();
        assert_eq!(idr.get(key1, &Guard::new()).unwrap(), 1);
        let key2 = idr.insert(2).unwrap();
        assert_eq!(idr.get(key2, &Guard::new()).unwrap(), 2);

        let idr1 = idr.clone();
        let t1 = thread::spawn(move || {
            let guard = Guard::new();
            assert_eq!(show!(idr1.get(key1, &guard)).unwrap(), 1);
            show!(idr1.remove(key1))
        });

        let idr2 = idr.clone();
        let t2 = thread::spawn(move || {
            let guard = Guard::new();
            assert_eq!(show!(idr2.get(key2, &guard)).unwrap(), 2);
            show!(idr2.remove(key2))
        });

        let r1 = t1.join().unwrap();
        let r2 = t2.join().unwrap();

        assert!(r1 && r2, "both threads removed entries");

        let guard = Guard::new();
        assert_eq!(idr.get(key0, &guard).unwrap(), 0);
        assert!(idr.get(key1, &guard).is_none());
        assert!(idr.get(key2, &guard).is_none());
    });
}

// Threads remove the same entry.
#[test]
fn racy_remove() {
    run_model(|| {
        let idr = Arc::new(Idr::default());
        let key = idr.insert(1).unwrap();

        let idr1 = idr.clone();
        let t1 = thread::spawn(move || {
            let guard = Guard::new();
            let seen = show!(idr1.get(key, &guard)).is_some();
            let removed = show!(idr1.remove(key));
            assert!(show!(idr1.get(key, &guard)).is_none());
            (seen, removed)
        });

        let idr2 = idr.clone();
        let t2 = thread::spawn(move || {
            let guard = Guard::new();
            let seen = show!(idr2.get(key, &guard)).is_some();
            let removed = show!(idr2.remove(key));
            assert!(show!(idr2.get(key, &guard)).is_none());
            (seen, removed)
        });

        let (s1, r1) = t1.join().unwrap();
        let (s2, r2) = t2.join().unwrap();

        assert!(s1 || s2, "at least one thread observed the entry");
        assert!(r1 ^ r2, "exactly one thread removed the entry");
        assert!(idr.get(key, &Guard::new()).is_none());
    });
}

// Threads remove the same entry.
// Additionally, one thread inserts a new entry, which can reuse the slot.
#[test]
fn racy_remove_reuse() {
    run_model(|| {
        let idr = Arc::new(Idr::default());
        let key = idr.insert(1).unwrap();

        let idr1 = idr.clone();
        let t1 = thread::spawn(move || {
            let removed = show!(idr1.remove(key));
            // It can reuse the same slot.
            show!(idr1.insert(2)).unwrap();
            removed
        });

        let idr2 = idr.clone();
        let t2 = thread::spawn(move || show!(idr2.remove(key)));

        let r1 = t1.join().unwrap();
        let r2 = t2.join().unwrap();

        assert!(r1 ^ r2, "exactly one thread removed the entry");
    });
}

// TODO: describe and expand after https://github.com/wvwwvwwv/scalable-concurrent-containers/issues/133
#[test]
fn racy_remove_guarded() {
    run_model(|| {
        let idr = Arc::new(Idr::default());

        let key = idr.insert(1).unwrap();
        let guard = Guard::new();
        let entry = idr.get(key, &guard).unwrap();
        assert_eq!(entry, 1);

        let idr1 = idr.clone();
        let t1 = thread::spawn(move || show!(idr1.remove(key)));
        let idr2 = idr.clone();
        let t2 = thread::spawn(move || show!(idr2.remove(key)));

        let r1 = t1.join().unwrap();
        let r2 = t2.join().unwrap();

        assert!(r1 ^ r2, "only one thread removed the entry");
        assert!(idr.get(key, &guard).is_none());
        assert_eq!(entry, 1);
    });
}

// One thread removes existing entries, and another thread reuses the slots.
#[test]
fn remove_reuse() {
    run_model(|| {
        let idr = Arc::new(Idr::<_, TinyConfig>::new());

        let key1 = idr.insert(1).unwrap();
        let key2 = idr.insert(2).unwrap();
        let key3 = idr.insert(3).unwrap();
        let key4 = idr.insert(4).unwrap();

        assert_eq!(idr.get(key1, &Guard::new()).unwrap(), 1);
        assert_eq!(idr.get(key2, &Guard::new()).unwrap(), 2);
        assert_eq!(idr.get(key3, &Guard::new()).unwrap(), 3);
        assert_eq!(idr.get(key4, &Guard::new()).unwrap(), 4);

        let idr1 = idr.clone();
        let t1 = thread::spawn(move || {
            assert!(show!(idr1.remove(key1)));
            assert!(show!(idr1.remove(key3)));
        });

        let idr2 = idr.clone();
        let t2 = thread::spawn(move || {
            let key1 = show!(idr2.insert(5)).unwrap();
            let key3 = show!(idr2.insert(6)).unwrap();
            (key1, key3)
        });
        t1.join().unwrap();
        let (key1, key3) = t2.join().unwrap();

        let guard = Guard::new();
        assert_eq!(idr.get(key1, &guard).unwrap(), 5);
        assert_eq!(idr.get(key2, &guard).unwrap(), 2);
        assert_eq!(idr.get(key3, &guard).unwrap(), 6);
        assert_eq!(idr.get(key4, &guard).unwrap(), 4);
    });
}

// One thread inserts an entry, and another thread removes it.
#[test]
fn insert_share_remove() {
    run_model(|| {
        let idr = Arc::new(Idr::default());
        let pair = Arc::new((Mutex::new(None), Condvar::new()));

        let idr2 = idr.clone();
        let pair2 = pair.clone();
        let remover = thread::spawn(move || {
            let (lock, cvar) = &*pair2;
            for i in 0..2 {
                let mut next = lock.lock().unwrap();
                while next.is_none() {
                    next = cvar.wait(next).unwrap();
                }
                let key = show!(next.take()).unwrap();
                let guard = Guard::new();
                assert_eq!(show!(idr2.get(key, &guard)).unwrap(), i);
                assert!(show!(idr2.remove(key)));
                cvar.notify_one();
            }
        });

        let (lock, cvar) = &*pair;
        for i in 0..2 {
            let key = idr.insert(i).unwrap();

            let mut next = lock.lock().unwrap();
            *next = Some(key);
            show!(cvar.notify_one());

            // Wait for the item to be removed.
            while next.is_some() {
                next = cvar.wait(next).unwrap();
            }

            let guard = Guard::new();
            assert!(show!(idr.get(key, &guard)).is_none());
        }

        remover.join().unwrap();
    });
}

// Iterating over the IDR while entries are being inserted.
#[test]
fn iter_insert() {
    run_model(|| {
        let idr = Arc::new(Idr::<_, TinyConfig>::new());

        let idr1 = idr.clone();
        let t1 = thread::spawn(move || {
            for i in 0..8 {
                show!(idr1.insert(i)).unwrap();
            }
        });

        let idr2 = idr.clone();
        let t2 = thread::spawn(move || {
            let guard = Guard::new();
            let count = show!(idr2.iter(&guard)).count();

            // Any subset of the inserted entries can be observed.
            // I'm not sure if we should provide stronger guarantees here.
            assert!((0..=8).contains(&count));
        });

        t1.join().unwrap();
        t2.join().unwrap();

        let guard = Guard::new();
        let mut all = idr.iter(&guard).map(|(_, v)| *v).collect::<Vec<_>>();
        all.sort_unstable();
        assert_eq!(all.len(), 8);

        for (i, v) in all.into_iter().enumerate() {
            assert_eq!(v, i);
        }
    });
}

// Iterating over the IDR while entries are being inserted and removed.
#[test]
fn iter_insert_remove() {
    run_model(|| {
        let idr = Arc::new(Idr::<_, TinyConfig>::new());

        let idr1 = idr.clone();
        let t1 = thread::spawn(move || {
            let key = show!(idr1.insert(1)).unwrap();
            idr1.remove(key);
        });

        let idr2 = idr.clone();
        let t2 = thread::spawn(move || {
            let key = show!(idr2.insert(2)).unwrap();
            idr2.remove(key);
        });

        let idr3 = idr.clone();
        let t3 = thread::spawn(move || {
            let guard = Guard::new();
            let all = show!(idr3.iter(&guard))
                .map(|(_, v)| *v)
                .collect::<Vec<_>>();

            match all.len() {
                0 => {}
                1 => assert!(all.contains(&1) || all.contains(&2)),
                2 => assert!(all.contains(&1) && all.contains(&2)),
                _ => unreachable!(),
            }
        });

        t1.join().unwrap();
        t2.join().unwrap();
        t3.join().unwrap();

        let guard = Guard::new();
        let count = idr.iter(&guard).count();
        assert_eq!(count, 0);
    });
}

// TODO: track allocations
#[test]
fn owned_entry_outlive_idr() {
    run_model(|| {
        let idr = Idr::default();
        let key1 = idr.insert(String::from("foo")).unwrap();
        let key2 = idr.insert(String::from("bar")).unwrap();

        let entry1_1 = idr.get_owned(key1).unwrap();
        let entry1_2 = idr.get_owned(key1).unwrap();
        let entry2 = idr.get_owned(key2).unwrap();
        drop(idr);

        let t1 = thread::spawn(move || {
            assert_eq!(&entry1_1, &String::from("foo"));
            show!(drop(entry1_1));
        });

        let t2 = thread::spawn(move || {
            assert_eq!(&entry2, &String::from("bar"));
            show!(drop(entry2));
        });

        t1.join().unwrap();
        t2.join().unwrap();

        assert_eq!(&entry1_2, &String::from("foo"));
    });
}

// Insert while removing the same entry under a key created from integer.
#[test]
fn ffi_insert_remove() {
    run_model(|| {
        let idr = Arc::new(Idr::<_, TinyConfig>::new());
        let fake_key = Key::try_from(0b10).unwrap();

        let idr1 = idr.clone();
        let t1 = thread::spawn(move || {
            let key = show!(idr1.insert(1)).unwrap();
            assert_eq!(key, fake_key);
        });

        let idr2 = idr.clone();
        let t2 = thread::spawn(move || {
            show!(idr2.remove(fake_key));
        });

        t1.join().unwrap();
        t2.join().unwrap();
    });
}

// Insert while getting the same entry under a key created from integer.
#[test]
fn ffi_insert_get() {
    run_model(|| {
        let idr = Arc::new(Idr::<_, TinyConfig>::new());
        let fake_key = Key::try_from(0b10).unwrap();

        let idr1 = idr.clone();
        let t1 = thread::spawn(move || {
            let key = show!(idr1.insert(1)).unwrap();
            assert_eq!(key, fake_key);
        });

        let idr2 = idr.clone();
        let t2 = thread::spawn(move || {
            let guard = Guard::new();
            show!(idr2.get(fake_key, &guard));
        });

        t1.join().unwrap();
        t2.join().unwrap();
    });
}

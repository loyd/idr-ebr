#![cfg(loom)]

use std::sync::Arc;

use loom::thread;

use idr_ebr::{Config, Guard, Idr};

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
    const INITIAL_PAGE_SIZE: u32 = 4;
    const RESERVED_BITS: u32 = 3;
}

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

        show!(idr.insert(5)).unwrap();
        show!(idr.insert(6)).unwrap();
        show!(idr.insert(7)).unwrap();
        show!(idr.insert(8)).unwrap();
        t1.join().unwrap();
    });
}

#[test]
fn concurrent_insert_remove() {
    run_model(|| {
        let idr = Arc::new(Idr::default());

        let idr1 = idr.clone();
        let t1 = thread::spawn(move || {
            let key = show!(idr1.insert(1)).unwrap();
            assert!(show!(idr1.remove(key)));
        });

        let key = show!(idr.insert(2)).unwrap();
        assert!(show!(idr.remove(key)));
        t1.join().unwrap();

        let key = idr.insert(0).unwrap();
        assert!(show!(idr.remove(key)));
    });
}

#[test]
fn concurrent_insert_remove_2() {
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

        let guard = Guard::new();
        let key = show!(idr.insert(3)).unwrap();
        assert_eq!(show!(idr.get(key, &guard)).unwrap(), 3);
        assert!(show!(idr.remove(key)));
        assert!(show!(idr.get(key, &guard)).is_none());

        let key = show!(idr.insert(4)).unwrap();
        assert_eq!(show!(idr.get(key, &guard)).unwrap(), 4);
        assert!(show!(idr.remove(key)));
        assert!(show!(idr.get(key, &guard)).is_none());

        t1.join().unwrap();
    });
}

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

#[test]
fn racy_remove_2() {
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

        assert!(r1 ^ r2, "only one thread has removed the entry");
        assert!(idr.get(key, &guard).is_none());
        assert_eq!(entry, 1);
    });
}

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

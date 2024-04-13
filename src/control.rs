use std::cell::Cell;

use fastrand::Rng;

use crate::loom::{
    sync::{
        atomic::{AtomicU32, Ordering},
        Mutex,
    },
    thread_local,
};

pub(crate) struct PageControl {
    // Used to synchronize page allocations.
    lock: Mutex<()>,

    // Used to distribute `Idr::insert()` across existing pages.
    // It improves performance by reducing contention.
    allocated: AtomicU32,
}

impl Default for PageControl {
    fn default() -> Self {
        Self {
            allocated: AtomicU32::new(0),
            lock: Mutex::new(()),
        }
    }
}

impl PageControl {
    pub(crate) fn get_or_lock<R>(
        &self,
        get: impl Fn() -> *const R,
        alloc: impl FnOnce(),
    ) -> *const R {
        let ptr = get();

        // The fast path, the page is already allocated.
        if !ptr.is_null() {
            return ptr;
        }

        let _guard = self.lock.lock().expect("lock poisoned");

        // Re-check if the page is allocated while acquiring the lock.
        let ptr = get();
        if !ptr.is_null() {
            return ptr;
        }

        // Actually allocate the page.
        alloc();
        let ptr = get();
        debug_assert!(!ptr.is_null());

        // Use `Relaxed` ordering here because no need to synchronize with `choose()`,
        // it's only for performance optimization and doesn't affect correctness.
        self.allocated.fetch_add(1, Ordering::Relaxed);

        ptr
    }

    pub(crate) fn choose<'a, P, R>(
        &self,
        pages: &'a [P],
        f: impl Fn(&'a P) -> Option<R>,
    ) -> Option<R> {
        // Use `Relaxed` ordering here because no need to synchronize with
        // `get_or_lock()`, it's only for performance optimization and doesn't
        // affect correctness either older or newer values are read.
        let allocated = self.allocated.load(Ordering::Relaxed);
        debug_assert!(allocated as usize <= pages.len());

        // Randomly choose a page to start from.
        // It helps to distribute the load more evenly and reduce contention.
        if allocated > 0 {
            let start_idx = gen_u32(allocated);

            for page in &pages[start_idx as usize..allocated as usize] {
                if let Some(ret) = f(page) {
                    return Some(ret);
                }
            }
        }

        // If we haven't found a page yet, try all pages.
        // Either we will find a page or create a new one.
        for page in pages {
            if let Some(ret) = f(page) {
                return Some(ret);
            }
        }

        None
    }

    pub(crate) fn allocated(&self) -> u32 {
        self.allocated.load(Ordering::Relaxed)
    }
}

thread_local! {
    static RNG: Cell<Rng> = Cell::new(Rng::with_seed(0xef6_f79e_d30b_a75a));
}

fn gen_u32(upper: u32) -> u32 {
    RNG.with(|cell| {
        let mut rng = cell.replace(Rng::with_seed(0));
        let ret = rng.u32(0..upper);
        cell.set(rng);
        ret
    })
}

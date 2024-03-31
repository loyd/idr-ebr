use crate::sync::{
    atomic::{AtomicU32, Ordering},
    Mutex,
};

pub(crate) struct PageControl {
    allocated: AtomicU32,

    // Used to synchronize page allocations.
    lock: Mutex<()>,
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

        let _guard = self.lock.lock();

        // Re-check if the page is allocated while acquiring the lock.
        let ptr = get();
        if !ptr.is_null() {
            return ptr;
        }

        // Actually allocate the page.
        alloc();
        let ptr = get();
        debug_assert!(!ptr.is_null());
        self.allocated.fetch_add(1, Ordering::Relaxed);
        ptr
    }

    pub(crate) fn choose<'a, P, R>(
        &self,
        pages: &'a [P],
        f: impl Fn(&'a P) -> Option<R>,
    ) -> Option<R> {
        let allocated = self.allocated.load(Ordering::Relaxed);
        debug_assert!(allocated as usize <= pages.len());

        if allocated > 0 {
            let start_idx = fastrand::u32(0..allocated);

            for page in &pages[start_idx as usize..allocated as usize] {
                if let Some(ret) = f(page) {
                    return Some(ret);
                }
            }
        }

        for page in pages {
            if let Some(ret) = f(page) {
                return Some(ret);
            }
        }

        None
    }
}

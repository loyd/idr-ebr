pub(crate) use self::inner::*;

#[cfg(not(loom))]
mod inner {
    pub(crate) use std::{alloc, sync, thread_local};

    pub(crate) use scc::ebr::AtomicShared;

    // See the mocked version below for details.
    pub(crate) struct ExclTrack;

    impl ExclTrack {
        #[inline(always)]
        pub(crate) fn new() -> Self {
            Self
        }

        #[inline(always)]
        pub(crate) fn ensure(&self) -> ExclGuard<'_> {
            ExclGuard(self)
        }
    }

    #[allow(dead_code)]
    pub(crate) struct ExclGuard<'a>(&'a ExclTrack);
}

#[cfg(loom)]
mod inner {
    pub(crate) use loom::{alloc, sync, thread_local};

    use scc::ebr;
    use sync::atomic::{AtomicPtr, Ordering};

    // TODO: `scc::ebr` doesn't support `loom` yet:
    // https://github.com/wvwwvwwv/scalable-concurrent-containers/issues/133
    //
    // Until it's implemented, we use a fake atomic pointer to make it visible to
    // `loom`. The loom tracks multiple versions, so we store them separetely.
    pub(crate) struct AtomicShared<T> {
        ptr: AtomicPtr<T>,
        // We don't use `loom::sync` here to avoid extra permutations.
        versions: std::sync::Mutex<Vec<ebr::Shared<T>>>,
    }

    impl<T> AtomicShared<T> {
        pub(crate) fn null() -> Self {
            Self {
                ptr: AtomicPtr::new(std::ptr::null_mut()),
                versions: <_>::default(),
            }
        }

        pub(crate) fn load<'g>(&self, order: Ordering, guard: &'g ebr::Guard) -> ebr::Ptr<'g, T> {
            let ptr = self.ptr.load(order);
            self.get_version(ptr)
                .map_or(ebr::Ptr::null(), |s| s.get_guarded_ptr(guard))
        }

        #[allow(clippy::type_complexity)]
        pub(crate) fn compare_exchange<'g>(
            &self,
            current: ebr::Ptr<'g, T>,
            new: (Option<ebr::Shared<T>>, ebr::Tag),
            success: Ordering,
            failure: Ordering,
            guard: &'g ebr::Guard,
        ) -> Result<
            (Option<ebr::Shared<T>>, ebr::Ptr<'g, T>),
            (Option<ebr::Shared<T>>, ebr::Ptr<'g, T>),
        > {
            assert_eq!(new.1, ebr::Tag::None);

            let current_ptr = current.as_ptr().cast_mut();
            let new_ptr = self.add_version(new.0);

            let handle = |ptr: *mut T| match self.get_version(ptr) {
                Some(shared) => {
                    let p = shared.get_guarded_ptr(guard);
                    (Some(shared), p)
                }
                None => (None, ebr::Ptr::null()),
            };

            self.ptr
                .compare_exchange(current_ptr, new_ptr, success, failure)
                .map(handle)
                .map_err(handle)
        }

        pub(crate) fn swap(
            &self,
            new: (Option<ebr::Shared<T>>, ebr::Tag),
            order: Ordering,
        ) -> (Option<ebr::Shared<T>>, ebr::Tag) {
            assert_eq!(new.1, ebr::Tag::None);

            let new_ptr = self.add_version(new.0);
            let old_ptr = self.ptr.swap(new_ptr, order);

            (self.get_version(old_ptr), ebr::Tag::None)
        }

        fn get_version(&self, ptr: *mut T) -> Option<ebr::Shared<T>> {
            if ptr.is_null() {
                return None;
            }

            let versions = self.versions.lock().unwrap();
            let shared = versions
                .iter()
                .rev()
                .find(|s| s.as_ptr() == ptr)
                .unwrap()
                .clone();

            Some(shared)
        }

        fn add_version(&self, shared: Option<ebr::Shared<T>>) -> *mut T {
            if let Some(shared) = shared {
                let ptr = shared.as_ptr().cast_mut();
                self.versions.lock().unwrap().push(shared);
                ptr
            } else {
                std::ptr::null_mut()
            }
        }
    }

    /// Panics if several threads try to access the same resource,
    /// which shouldn't be accessed concurrently.
    /// We don't use `loom::sync` here to avoid extra permutations.
    pub(crate) struct ExclTrack(std::sync::atomic::AtomicBool);

    impl ExclTrack {
        pub(crate) fn new() -> Self {
            Self(std::sync::atomic::AtomicBool::new(false))
        }

        #[track_caller]
        pub(crate) fn ensure(&self) -> ExclGuard<'_> {
            assert!(
                !self.0.swap(true, Ordering::Relaxed),
                "unexpected concurrent access"
            );

            ExclGuard(self)
        }
    }

    pub(crate) struct ExclGuard<'a>(&'a ExclTrack);

    impl Drop for ExclGuard<'_> {
        fn drop(&mut self) {
            assert!(self.0 .0.swap(false, Ordering::Relaxed));
        }
    }
}

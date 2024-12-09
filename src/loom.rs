pub(crate) use self::inner::*;

#[cfg(not(all(idr_ebr_loom, feature = "loom")))]
mod inner {
    pub(crate) use std::{alloc, sync, thread_local};

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

#[cfg(all(idr_ebr_loom, feature = "loom"))]
mod inner {
    pub(crate) use loom::{alloc, sync, thread_local};

    use sync::atomic::Ordering;

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

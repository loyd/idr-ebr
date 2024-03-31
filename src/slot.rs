use std::marker::PhantomData;

use scc::ebr;

use crate::{
    config::Config,
    key::{Generation, Key},
    sync::atomic::{AtomicU32, Ordering},
};

// TODO: use loom Track
pub(crate) struct Slot<T, C> {
    generation: AtomicU32,
    next_free: AtomicU32, // MAX means no next
    data: ebr::AtomicShared<T>,
    _config: PhantomData<C>,
}

impl<T: 'static, C: Config> Slot<T, C> {
    pub(crate) fn new(next_free: u32) -> Self {
        Self {
            generation: AtomicU32::new(0),
            next_free: AtomicU32::new(next_free),
            data: ebr::AtomicShared::null(),
            _config: PhantomData,
        }
    }

    pub(crate) fn init(&self, value: T) {
        let pair = (Some(ebr::Shared::new(value)), ebr::Tag::None);
        let (old_data, _) = self.data.swap(pair, Ordering::Release);
        debug_assert!(old_data.is_none());
    }

    pub(crate) fn uninit(&self) -> bool {
        let (unreachable, _) = self.data.swap((None, ebr::Tag::None), Ordering::Release);

        if let Some(unreachable) = unreachable {
            // For now, `impl Drop for Shared` uses a special guard, which doesn't clean up.
            // It can cause OOM if a thread is alive for a long time and doesn't use a
            // normal guard via `Idr::get()` or directly (see `insert_remove` benchmark).
            // TODO: create an issue in scc.
            let _ = unreachable.release(&ebr::Guard::new());
        } else {
            return false;
        }

        let gen = self.generation.load(Ordering::Relaxed);
        let new_gen = Generation::<C>::new(gen).inc();
        self.generation.store(new_gen.to_u32(), Ordering::Release);

        true
    }

    pub(crate) fn generation(&self) -> Generation<C> {
        let gen = self.generation.load(Ordering::Acquire);
        Generation::<C>::new(gen)
    }

    pub(crate) fn next_free(&self) -> u32 {
        self.next_free.load(Ordering::Acquire)
    }

    pub(crate) fn set_next_free(&self, index: u32) {
        self.next_free.store(index, Ordering::Release);
    }

    pub(crate) fn get<'g>(&self, key: Key, guard: &'g ebr::Guard) -> ebr::Ptr<'g, T> {
        let data = self.data.load(Ordering::Acquire, guard);
        let generation = self.generation.load(Ordering::Acquire);

        if key.generation::<C>() != Generation::<C>::new(generation) {
            return ebr::Ptr::null();
        }

        data
    }
}

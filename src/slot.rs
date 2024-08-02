use std::marker::PhantomData;

use crate::{
    config::Config,
    key::{Generation, Key},
    loom::{
        sync::atomic::{AtomicU32, Ordering},
        AtomicShared, ExclTrack,
    },
    EbrGuard,
};

pub(crate) struct Slot<T, C> {
    generation: AtomicU32,
    next_free: AtomicU32, // MAX means no next
    data: AtomicShared<T>,
    exclusive: ExclTrack, // loom only
    _config: PhantomData<C>,
}

impl<T: 'static, C: Config> Slot<T, C> {
    pub(crate) fn new(next_free: u32) -> Self {
        Self {
            generation: AtomicU32::new(0),
            next_free: AtomicU32::new(next_free),
            data: AtomicShared::null(),
            exclusive: ExclTrack::new(),
            _config: PhantomData,
        }
    }

    pub(crate) fn init(&self, value: T) {
        let _track = self.exclusive.ensure();
        let pair = (Some(sdd::Shared::new(value)), sdd::Tag::None);

        // It's impossible to reach this point for the same slot concurrently.
        // Thus, we can use `swap` (`xchgl` on x86-64) here as a cheaper alternative to
        // `compare_exchange` (`lock cmpxchgl` on x86-64).
        // NOTE: `sdd::AtomicShared` doesn't support `store()`.
        let (old_data, _) = self.data.swap(pair, Ordering::Release);
        debug_assert!(old_data.is_none());
    }

    pub(crate) fn uninit(&self, key: Key) -> bool {
        // For now, `impl Drop for Shared` uses a special guard, which doesn't clean up.
        // It can cause OOM if a thread is alive for a long time and doesn't use a
        // normal guard via `Idr::get()` or directly (see `insert_remove` benchmark).
        // TODO: create an issue in sdd. However, it's still required for `get()`.
        let guard = EbrGuard::new();

        // Check if this slot corresponds to the key.
        let ptr = self.get(key, &guard);
        if ptr.is_null() {
            return false;
        }

        // Try to replace the data pointer with the null pointer
        // in order to make it unreachable via IDR for other threads.
        //
        // It fails if another thread removed or even replaced the same slot
        // concurrently after this one called `get()` above.
        //
        // There is no ABA problem with the data pointer here because
        // the data pointer cannot be reused until the EBR guard is dropped.
        let Ok((unreachable, _)) = self.data.compare_exchange(
            ptr,
            (None, sdd::Tag::None),
            Ordering::AcqRel,
            Ordering::Relaxed,
            &guard.0,
        ) else {
            // If either the slot was removed or replaced, simply return.
            // We don't need to retry or check generation in this case.
            return false;
        };

        // It's impossible to reach this point for the same slot concurrently.
        let _track = self.exclusive.ensure();
        let _ = unreachable.unwrap().release();

        // We can use `store` instead of CAS here because:
        // * This code is executed only by one thread.
        // * This is the only place where the generation is changed.
        let new_generation = key.generation::<C>().inc().to_u32();
        self.generation.store(new_generation, Ordering::Relaxed);

        true
    }

    pub(crate) fn generation(&self) -> Generation<C> {
        let gen = self.generation.load(Ordering::Relaxed);
        Generation::<C>::new(gen)
    }

    pub(crate) fn next_free(&self) -> u32 {
        self.next_free.load(Ordering::Acquire)
    }

    pub(crate) fn set_next_free(&self, index: u32) {
        self.next_free.store(index, Ordering::Release);
    }

    pub(crate) fn get<'g>(&self, key: Key, guard: &'g EbrGuard) -> sdd::Ptr<'g, T> {
        let data = self.data.load(Ordering::Acquire, &guard.0);
        let generation = self.generation.load(Ordering::Relaxed);

        if key.generation::<C>() != Generation::<C>::new(generation) {
            return sdd::Ptr::null();
        }

        data
    }
}

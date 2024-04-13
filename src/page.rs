use std::{ptr, slice};

use scc::ebr;

use crate::{
    config::Config,
    control::PageControl,
    key::{Key, PageNo},
    loom::{
        alloc,
        sync::atomic::{AtomicPtr, AtomicU32, Ordering},
    },
    slot::Slot,
    BorrowedEntry,
};

// === Page ===

pub(crate) struct Page<T, C> {
    start_slot_id: u32,
    capacity: u32,
    slots: AtomicPtr<Slot<T, C>>,
    free_head: AtomicU32, // MAX means no free slots
}

impl<T: 'static, C: Config> Page<T, C> {
    pub(crate) fn new(page_no: PageNo<C>) -> Self {
        Self {
            start_slot_id: page_no.start_slot_id(),
            capacity: page_no.capacity(),
            slots: AtomicPtr::new(ptr::null_mut()),
            free_head: AtomicU32::new(0),
        }
    }

    /// # Safety
    ///
    /// The provided slot must belong to this page.
    pub(crate) unsafe fn add_free(&self, slot: &Slot<T, C>) {
        let slots_ptr = self.slots.load(Ordering::Relaxed);
        debug_assert!(!slots_ptr.is_null());

        let mut free_head = self.free_head.load(Ordering::Acquire);
        loop {
            slot.set_next_free(free_head);

            // SAFETY: Derived from the invariant that the slot belongs to this page.
            let slot_index = (slot as *const Slot<T, C>).offset_from(slots_ptr);
            debug_assert!((0isize..(1 << 31)).contains(&slot_index));

            // It never truncates, because the index is less than 2^31.
            // This is because the slot id includes a bit of a page.
            #[allow(clippy::cast_sign_loss)]
            let slot_index = slot_index as u32;
            debug_assert!(slot_index < self.capacity);

            // TODO: ordering
            if let Err(new_free_head) = self.free_head.compare_exchange(
                free_head,
                slot_index,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                free_head = new_free_head;
            } else {
                break;
            }
        }
    }

    pub(crate) fn reserve(&self, page_control: &PageControl) -> Option<(Key, &Slot<T, C>)> {
        let slots_ptr =
            page_control.get_or_lock(|| self.slots.load(Ordering::Acquire), || self.allocate());

        let mut free_head = self.free_head.load(Ordering::Acquire);
        let (slot_index, slot) = loop {
            if free_head == u32::MAX {
                return None;
            }

            debug_assert!(free_head < self.capacity);

            // SAFETY: Both the starting and resulting pointer is in bounds of the same
            // allocated object, because `free_head` is always less than `self.capacity`.
            let slot = unsafe { &*slots_ptr.add(free_head as usize) };

            let next_free_head = slot.next_free();
            debug_assert!(next_free_head == u32::MAX || next_free_head < self.capacity);

            // TODO: ordering
            if let Err(new_free_head) = self.free_head.compare_exchange(
                free_head,
                next_free_head,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                free_head = new_free_head;
            } else {
                break (free_head, slot);
            }
        };

        // SAFETY: `slot_id` is always non-zero, because it includes a bit of a page.
        let key = unsafe { Key::new_unchecked(self.start_slot_id + slot_index, slot.generation()) };

        Some((key, slot))
    }

    pub(crate) fn remove(&self, key: Key) -> bool {
        let slots_ptr = self.slots.load(Ordering::Relaxed);
        if slots_ptr.is_null() {
            return false;
        }

        let slot_id = key.slot_id::<C>();
        let slot_index = slot_id - self.start_slot_id;
        debug_assert!(slot_index < self.capacity);

        // SAFETY: Both the starting and resulting pointer is in bounds of the same
        // allocated object, because `slot_id` belongs to this page.
        let slot = unsafe { &*slots_ptr.add(slot_index as usize) };
        if !slot.uninit(key) {
            return false;
        }

        // SAFETY: The slot belongs to this page.
        unsafe { self.add_free(slot) };
        true
    }

    pub(crate) fn get<'g>(&self, key: Key, guard: &'g ebr::Guard) -> Option<BorrowedEntry<'g, T>> {
        let slots_ptr = self.slots.load(Ordering::Relaxed);
        if slots_ptr.is_null() {
            return None;
        }

        let slot_index = key.slot_id::<C>() - self.start_slot_id;
        debug_assert!(slot_index < self.capacity);

        // SAFETY: Both the starting and resulting pointer is in bounds of the same
        // allocated object, because `slot_index` belongs to this page.
        let slot = unsafe { &*slots_ptr.add(slot_index as usize) };
        BorrowedEntry::new(slot.get(key, guard))
    }

    /// Iterates over occupied slots, or `None` if the page isn't allocated.
    #[allow(clippy::iter_not_returning_iterator)]
    pub(crate) fn iter<'g>(&self, guard: &'g ebr::Guard) -> Option<Iter<'g, '_, T, C>> {
        let slots_ptr = self.slots.load(Ordering::Relaxed);
        if slots_ptr.is_null() {
            return None;
        }

        // SAFETY: Slots are properly initialized.
        let slots = unsafe { slice::from_raw_parts(slots_ptr, self.capacity as usize) };

        Some(Iter {
            slots,
            // It never underflows, because slot ids are non-zero.
            prev_slot_id: self.start_slot_id - 1,
            guard,
        })
    }

    #[cold]
    #[inline(never)]
    fn allocate(&self) {
        debug_assert!(self.slots.load(Ordering::Relaxed).is_null());

        let layout =
            alloc::Layout::array::<Slot<T, C>>(self.capacity as usize).expect("invalid layout");
        assert_ne!(layout.size(), 0);

        // SAFETY: `layout` is valid and non-zero because of assertions above.
        let slots_ptr = unsafe { alloc::alloc(layout) };

        assert!(!slots_ptr.is_null(), "failed to allocate memory");

        #[allow(clippy::cast_ptr_alignment)] // ensured by `layout` above
        let slots_ptr = slots_ptr.cast::<Slot<T, C>>();

        for slot_index in 0..self.capacity {
            // SAFETY: Both the starting and resulting pointer is in bounds of the same
            // allocated object, because `slot_index` belongs to this page.
            let slot_ptr = unsafe { slots_ptr.add(slot_index as usize) };

            // It never overflows, because the index is less than 2^31.
            // This is because the slot id includes a bit of a page.
            let next_free = if slot_index + 1 < self.capacity {
                slot_index + 1
            } else {
                u32::MAX
            };

            let slot = Slot::new(next_free);

            // SAFETY: The slot is properly aligned.
            unsafe { slot_ptr.write(slot) };
        }

        debug_assert!(self.slots.load(Ordering::Relaxed).is_null());
        self.slots.store(slots_ptr, Ordering::Release);
    }
}

impl<T, C> Drop for Page<T, C> {
    fn drop(&mut self) {
        let slots_ptr = self.slots.load(Ordering::Relaxed);

        if slots_ptr.is_null() {
            return;
        }

        // Call destructors.
        for slot_index in 0..self.capacity {
            // SAFETY: Both the starting and resulting pointer is in bounds of the same
            // allocated object, because `slot_index` belongs to this page.
            let slot_ptr = unsafe { slots_ptr.add(slot_index as usize) };

            // SAFETY:
            // * the slot is properly aligned
            // * this pointer is non-null
            // * data cannot be accessed outside of the destructor
            unsafe { slot_ptr.drop_in_place() };
        }

        // Deallocate memory.
        let layout =
            alloc::Layout::array::<Slot<T, C>>(self.capacity as usize).expect("invalid layout");

        // SAFETY:
        // * a block of memory currently allocated via this allocator
        // * layout is the same layout that was used to allocate that block of memory
        unsafe { alloc::dealloc(slots_ptr.cast::<u8>(), layout) };
    }
}

// === Iter ===

/// Iterates over occupied slots.
#[must_use]
pub(crate) struct Iter<'g, 's, T, C> {
    slots: &'s [Slot<T, C>],
    prev_slot_id: u32,
    guard: &'g ebr::Guard,
}

impl<'g, 's, T: 'static, C: Config> Iterator for Iter<'g, 's, T, C> {
    type Item = (Key, BorrowedEntry<'g, T>);

    fn next(&mut self) -> Option<Self::Item> {
        while let Some((slot, rest)) = self.slots.split_first() {
            // It never overflows, because it contains the index of a previous slot.
            self.prev_slot_id += 1;
            self.slots = rest;

            // SAFETY: `slot_id` is always non-zero, because it includes a bit of a page.
            let key = unsafe { Key::new_unchecked(self.prev_slot_id, slot.generation()) };
            let ptr = slot.get(key, self.guard);

            if let Some(entry) = BorrowedEntry::new(ptr) {
                return Some((key, entry));
            }
        }

        None
    }
}

impl<T: 'static, C: Config> std::iter::FusedIterator for Iter<'_, '_, T, C> {}

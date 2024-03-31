use std::{alloc, ptr};

use scc::ebr;

use crate::{
    config::Config,
    key::{Key, PageNo},
    slot::Slot,
    sync::{
        atomic::{AtomicPtr, AtomicU32, Ordering},
        Mutex,
    },
};

pub(crate) struct Page<T, C> {
    start_slot_id: u32,
    capacity: u32,
    slots: AtomicPtr<Slot<T, C>>, // TODO: just *const?
    free_head: AtomicU32,         // MAX means no free slots
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

    pub(crate) fn add_free(&self, slot: &Slot<T, C>) {
        let slots_ptr = self.slots.load(Ordering::Relaxed);
        debug_assert!(!slots_ptr.is_null());

        let mut free_head = self.free_head.load(Ordering::Acquire);
        loop {
            slot.set_next_free(free_head);

            let slot_index = unsafe { (slot as *const Slot<T, C>).offset_from(slots_ptr) };
            debug_assert!(0 <= slot_index && slot_index < self.capacity as isize);

            // TODO: ordering
            if let Err(new_free_head) = self.free_head.compare_exchange(
                free_head,
                slot_index as u32,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                free_head = new_free_head;
            } else {
                break;
            }
        }
    }

    pub(crate) fn reserve(&self, page_alloc_lock: &Mutex<()>) -> Option<(Key, &Slot<T, C>)> {
        let mut slots_ptr = self.slots.load(Ordering::Relaxed);

        if slots_ptr.is_null() {
            slots_ptr = self.allocate(page_alloc_lock);
        }

        let mut free_head = self.free_head.load(Ordering::Acquire);
        let (slot_index, slot) = loop {
            if free_head == u32::MAX {
                return None;
            }

            debug_assert!(free_head < self.capacity);

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

        let slot = unsafe { &*slots_ptr.add(slot_index as usize) };
        if !slot.uninit() {
            return false;
        }

        self.add_free(slot);
        true
    }

    pub(crate) fn get<'g>(&self, key: Key, guard: &'g ebr::Guard) -> ebr::Ptr<'g, T> {
        let slots_ptr = self.slots.load(Ordering::Relaxed);
        if slots_ptr.is_null() {
            return ebr::Ptr::null();
        }

        let slot_index = key.slot_id::<C>() - self.start_slot_id;
        debug_assert!(slot_index < self.capacity);

        let slot = unsafe { &*slots_ptr.add(slot_index as usize) };
        slot.get(key, guard)
    }

    #[cold]
    #[inline(never)]
    fn allocate(&self, page_alloc_lock: &Mutex<()>) -> *mut Slot<T, C> {
        let _guard = page_alloc_lock.lock();

        let slots_ptr = self.slots.load(Ordering::Relaxed);
        if !slots_ptr.is_null() {
            return slots_ptr;
        }

        let layout =
            alloc::Layout::array::<Slot<T, C>>(self.capacity as usize).expect("invalid layout");
        let slots_ptr = unsafe { alloc::alloc(layout) };

        if slots_ptr.is_null() {
            panic!("failed to allocate memory");
        }

        let slots_ptr = slots_ptr.cast::<Slot<T, C>>();

        for slot_index in 0..self.capacity {
            let slot_ptr = unsafe { slots_ptr.add(slot_index as usize) };

            // TODO: comment it never overflows
            let next_free = if slot_index + 1 < self.capacity {
                slot_index + 1
            } else {
                u32::MAX
            };

            let slot = Slot::new(next_free);
            unsafe { slot_ptr.write(slot) };
        }

        self.slots.store(slots_ptr, Ordering::Relaxed);
        slots_ptr
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
            let slot_ptr = unsafe { slots_ptr.add(slot_index as usize) };
            unsafe { ptr::drop_in_place(slot_ptr) };
        }

        // Deallocate memory.
        let layout =
            alloc::Layout::array::<Slot<T, C>>(self.capacity as usize).expect("invalid layout");
        unsafe { alloc::dealloc(slots_ptr as *mut u8, layout) };
    }
}

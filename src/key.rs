use std::{marker::PhantomData, num::NonZeroU64};

use crate::config::{Config, ConfigPrivate};

// === Key ===

/// Represents a key in the IDR.
///
/// Properties:
/// * non-zero.
/// * always 64bit, even on 32bit platforms.
/// * contains reserved bits, generation, page and slot indexes.
///
/// See [`Config`] for more details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Key(NonZeroU64);

impl Key {
    /// # Safety
    ///
    /// Both parameters cannot be zero.
    pub(crate) unsafe fn new_unchecked<C: Config>(slot_id: u32, generation: Generation<C>) -> Self {
        debug_assert!(slot_id > 0);
        let raw = u64::from(generation.to_u32()) << C::SLOT_BITS | u64::from(slot_id);
        Self(NonZeroU64::new_unchecked(raw))
    }

    pub(crate) fn page_no<C: Config>(self) -> PageNo<C> {
        let slot_id = self.slot_id::<C>();

        // Let's assume (for example):
        // * width = 8bits
        // * ips (initial page size) = 4
        //
        //   repr    page  index    slot
        // +--------+----+-------+--------+
        //  000001xx  0    0..=3   0..=3
        //  00001xxx  1    0..=7   4..=11
        //  0001xxxx  2    0..=15  12..=27
        //  001xxxxx  3    0..=31  28..=59
        //  01xxxxxx  4    0..=63  60..=123
        //  1xxxxxxx  5    0..=127 124..=251
        //
        // Pros:
        // * less operations on read by key than in sharded-slab
        // * repr != 0 => key is non-zero
        //
        // Cons:
        // * total capacity is less (by ips) compared to sharded-slab
        //
        // page = width - lz(repr >> log2(ips))
        //      = (width - tz(ips) - 1) - lz(repr)   [2ops]
        //        '-------------------'
        //              constant
        //
        // index = repr - (1 << (tz(ips) + page))    [1op]
        //                '---------------------'
        //                         cached

        let page_no = 32 - C::INITIAL_PAGE_SIZE.trailing_zeros() - 1 - slot_id.leading_zeros();

        PageNo::new(page_no)
    }

    pub(crate) fn slot_id<C: Config>(self) -> u32 {
        self.0.get() as u32 & C::SLOT_MASK
    }

    pub(crate) fn generation<C: Config>(self) -> Generation<C> {
        let gen = (self.0.get() >> C::SLOT_BITS) as u32 & C::GENERATION_MASK;
        Generation::new(gen)
    }
}

impl From<NonZeroU64> for Key {
    fn from(raw: NonZeroU64) -> Self {
        Self(raw)
    }
}

impl From<Key> for NonZeroU64 {
    fn from(key: Key) -> NonZeroU64 {
        key.0
    }
}

// === PageNo ===

#[repr(transparent)]
pub(crate) struct PageNo<C> {
    value: u32,
    _config: PhantomData<C>,
}

impl<C> Copy for PageNo<C> {}

impl<C> Clone for PageNo<C> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<C: Config> PageNo<C> {
    pub(crate) fn new(value: u32) -> Self {
        Self {
            value,
            _config: PhantomData,
        }
    }

    pub(crate) fn to_usize(self) -> usize {
        self.value as usize
    }

    pub(crate) fn start_slot_id(self) -> u32 {
        let shift = C::INITIAL_PAGE_SIZE.trailing_zeros() + self.value;
        1 << shift
    }

    pub(crate) fn capacity(self) -> u32 {
        C::INITIAL_PAGE_SIZE * 2u32.pow(self.value)
    }
}

impl<C> PartialEq for PageNo<C> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

// === Generation ===

#[repr(transparent)]
pub(crate) struct Generation<C> {
    value: u32,
    _config: PhantomData<C>,
}

impl<C> Copy for Generation<C> {}

impl<C> Clone for Generation<C> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<C: Config> Generation<C> {
    pub(crate) fn new(value: u32) -> Self {
        Self {
            value,
            _config: PhantomData,
        }
    }

    pub(crate) fn to_u32(self) -> u32 {
        self.value
    }

    pub(crate) fn inc(self) -> Self {
        Self {
            value: (self.value + 1) & C::GENERATION_MASK,
            _config: PhantomData,
        }
    }
}

impl<C> PartialEq for Generation<C> {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

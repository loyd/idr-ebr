use std::{fmt, mem, ops::Deref};

use crate::{
    config::Config,
    key::Key,
    page::{self, Page},
    slot::Slot,
    EbrGuard,
};

// === VacantEntry ===

/// A handle to a vacant entry in an IDR.
///
/// It allows constructing values with the key that they will be assigned to.
///
/// See [`Idr::vacant_entry()`] for more details.
///
/// [`Idr::vacant_entry()`]: crate::Idr::vacant_entry
#[must_use]
pub struct VacantEntry<'s, T: 'static, C: Config> {
    page: &'s Page<T, C>,
    slot: &'s Slot<T, C>,
    key: Key,
}

impl<'s, T: 'static, C: Config> VacantEntry<'s, T, C> {
    pub(crate) fn new(page: &'s Page<T, C>, slot: &'s Slot<T, C>, key: Key) -> Self {
        Self { page, slot, key }
    }

    /// Returns the key at which this entry will be inserted.
    ///
    /// An entry stored in this entry will be associated with this key.
    #[must_use]
    #[inline]
    pub fn key(&self) -> Key {
        self.key
    }

    /// Inserts a value in the IDR.
    ///
    /// This method is wait-free.
    ///
    /// To get the key at which this value will be inserted, use
    /// [`VacantEntry::key()`] prior to calling this method.
    #[inline]
    pub fn insert(self, value: T) {
        self.slot.init(value);
        mem::forget(self);
    }
}

impl<T: 'static, C: Config> Drop for VacantEntry<'_, T, C> {
    #[inline]
    fn drop(&mut self) {
        // SAFETY: The slot belongs to this page by construction.
        unsafe { self.page.add_free(self.slot) };
    }
}

impl<T, C: Config> fmt::Debug for VacantEntry<'_, T, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("VacantEntry")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

// === BorrowedEntry ===

/// A borrowed handle that allows access to an occupied entry in an IDR.
///
/// See [`Idr::get()`] for more details.
///
/// [`Idr::get()`]: crate::Idr::get
#[must_use]
pub struct BorrowedEntry<'g, T>(sdd::Ptr<'g, T> /* non-null */);

impl<'g, T> BorrowedEntry<'g, T> {
    pub(crate) fn new(ptr: sdd::Ptr<'g, T>) -> Option<Self> {
        (!ptr.is_null()).then_some(Self(ptr))
    }

    /// Creates an owned handle to the entry.
    ///
    /// This method is lock-free, but it modifies the memory by incrementing the
    /// reference counter.
    ///
    /// See [`OwnedEntry`] for more details.
    #[inline]
    pub fn to_owned(self) -> OwnedEntry<T> {
        let maybe_shared = self.0.get_shared();

        // SAFETY: The pointer is non-null, checked in `new()`.
        OwnedEntry(unsafe { maybe_shared.unwrap_unchecked() })
    }

    #[doc(hidden)]
    #[deprecated(note = "use `to_owned()` instead")]
    #[inline]
    pub fn into_owned(self) -> OwnedEntry<T> {
        self.to_owned()
    }
}

impl<T> Copy for BorrowedEntry<'_, T> {}

impl<T> Clone for BorrowedEntry<'_, T> {
    #[inline]
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Deref for BorrowedEntry<'_, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        let maybe_ref = self.0.as_ref();

        // SAFETY: The pointer is non-null, checked in `new()`.
        unsafe { maybe_ref.unwrap_unchecked() }
    }
}

impl<T: fmt::Debug> fmt::Debug for BorrowedEntry<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&**self, f)
    }
}

impl<T: PartialEq<T>> PartialEq<T> for BorrowedEntry<'_, T> {
    #[inline]
    fn eq(&self, other: &T) -> bool {
        (**self).eq(other)
    }
}

// === OwnedEntry ===

/// An owned handle that allows access to an occupied entry in an IDR.
///
/// See [`Idr::get_owned()`] for more details.
///
/// [`Idr::get_owned()`]: crate::Idr::get_owned
#[must_use]
pub struct OwnedEntry<T>(sdd::Shared<T>);

impl<T> Clone for OwnedEntry<T> {
    #[inline]
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}

impl<T> Deref for OwnedEntry<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: fmt::Debug> fmt::Debug for OwnedEntry<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&*self.0, f)
    }
}

impl<T: PartialEq<T>> PartialEq<T> for OwnedEntry<T> {
    #[inline]
    fn eq(&self, other: &T) -> bool {
        self.0.eq(other)
    }
}

// === Iter ===

/// A fused iterator over all occupied entries in the IDR.
///
/// See [`Idr::iter()`] for more details.
///
/// [`Idr::iter()`]: crate::Idr::iter
#[must_use]
pub struct Iter<'g, 's, T, C> {
    pages: &'s [Page<T, C>],
    slots: Option<page::Iter<'g, 's, T, C>>,
    guard: &'g EbrGuard,
}

impl<'g, 's, T: 'static, C: Config> Iter<'g, 's, T, C> {
    pub(crate) fn new(pages: &'s [Page<T, C>], guard: &'g EbrGuard) -> Self {
        let (first, rest) = pages.split_first().expect("invalid MAX_PAGES");

        Self {
            pages: rest,
            slots: first.iter(guard),
            guard,
        }
    }
}

impl<'g, 's, T: 'static, C: Config> Iterator for Iter<'g, 's, T, C> {
    type Item = (Key, BorrowedEntry<'g, T>);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let slots = self.slots.as_mut()?;

            if let Some(pair) = slots.next() {
                return Some(pair);
            }

            let (slots, rest) = self
                .pages
                .split_first()
                .map(|(next, rest)| (next.iter(self.guard), rest))
                .unwrap_or_default();

            self.pages = rest;
            self.slots = slots;
        }
    }
}

impl<T: 'static, C: Config> std::iter::FusedIterator for Iter<'_, '_, T, C> {}

impl<T, C> fmt::Debug for Iter<'_, '_, T, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Iter").finish_non_exhaustive()
    }
}

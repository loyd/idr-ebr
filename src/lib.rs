//! An efficient concurrent ID to object resolver.
//!
//! An IDR (IDentifier Resolver) provides a way to efficiently and concurrently
//! map integer IDs to references to objects. It's particularly useful in
//! scenarios where you need to quickly find objects based on their ID.
//!
//! The main structure of this crate is [`Idr`].
//!
//! TODO

#![warn(rust_2018_idioms, unreachable_pub, missing_docs)]

use std::{fmt, mem, ops::Deref};

use scc::ebr;

use self::{config::ConfigPrivate, key::PageNo, page::Page, slot::Slot, sync::Mutex};

mod config;
mod key;
mod page;
mod slot;
mod sync;

pub use self::{
    config::{Config, DefaultConfig},
    key::Key,
};

// === Idr ===

/// An IDR (IDentifier Resolver) provides a way to efficiently and concurrently
/// map integer IDs to references to objects. It's particularly useful in
/// scenarios where you need to quickly find objects based on their ID. This
/// structure is designed to be highly efficient in terms of both speed and
/// memory usage.
pub struct Idr<T, C = DefaultConfig> {
    // TODO: flatten
    pages: Box<[Page<T, C>]>,
    // Used to synchronize page allocations.
    page_alloc_lock: Mutex<()>,
}

impl<T: 'static> Default for Idr<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: 'static, C: Config> Idr<T, C> {
    /// Returns a new IDR with the provided configuration parameters.
    pub fn new() -> Self {
        // Perform compile-time postmono checks.
        assert!(C::ENSURE_VALID);

        let pages = (0..C::MAX_PAGES).map(PageNo::new).map(Page::new).collect();
        Self {
            pages,
            page_alloc_lock: Mutex::new(()),
        }
    }

    /// Inserts a value into the IDR, returning the key at which that
    /// value was inserted. This key can then be used to access the entry.
    ///
    /// This method is, usually, lock-free. However, it can block if a new page
    /// should be allocated. Once allocated, the page is never deallocated.
    /// Thus, it can block no more than [`Config::MAX_PAGES`] times.
    ///
    /// Returns `None` if there is no more space in the IDR,
    /// and no items can be added until some are removed.
    ///
    /// # Example
    /// ```
    /// # use idr_ebr::Idr;
    /// let idr = Idr::default();
    ///
    /// let key = idr.insert("foo").unwrap();
    /// assert_eq!(idr.get(key).unwrap(), "foo");
    /// ```
    #[inline]
    pub fn insert(&self, value: T) -> Option<Key> {
        self.vacant_entry().map(|entry| {
            let key = entry.key();
            entry.insert(value);
            key
        })
    }

    /// Returns a handle to a vacant entry allowing for further manipulation.
    ///
    /// This method is, usually, lock-free. However, it can block if a new page
    /// should be allocated. Once allocated, the page is never deallocated.
    /// Thus, it can block no more than [`Config::MAX_PAGES`] times.
    ///
    /// This method is useful when creating values that must contain their
    /// IDR key. The returned [`VacantEntry`] reserves a slot in the IDR and
    /// is able to return the key of the entry.
    ///
    /// Returns `None` if there is no more space in the IDR,
    /// and no items can be added until some are removed.
    ///
    /// # Example
    /// ```
    /// # use idr_ebr::Idr;
    /// let idr = Idr::default();
    ///
    /// let key = {
    ///     let entry = idr.vacant_entry().unwrap();
    ///     let key = entry.key();
    ///     entry.insert((key, "foo"));
    ///     key
    /// };
    ///
    /// assert_eq!(idr.get(key).unwrap().0, key);
    /// assert_eq!(idr.get(key).unwrap().1, "foo");
    /// ```
    #[inline]
    pub fn vacant_entry(&self) -> Option<VacantEntry<'_, T, C>> {
        for page in self.pages.iter() {
            if let Some((key, slot)) = page.reserve(&self.page_alloc_lock) {
                return Some(VacantEntry { page, slot, key });
            }
        }

        None
    }

    /// Removes the entry at the given key in the IDR, returning `true` if a
    /// value was present at the moment of the removal.
    ///
    /// This method is lock-free.
    ///
    /// The removed entry becomes unreachable for getting instantly,
    /// but it still can be accessed using existing handles.
    ///
    /// An object behind the entry is not actually dropped until all handles are
    /// dropped and EBR garbage is cleaned up.
    ///
    /// # Example
    /// ```
    /// # use idr_ebr::Idr;
    /// let idr = Idr::default();
    /// let key = idr.insert("foo").unwrap();
    ///
    /// let entry = idr.get(key).unwrap();
    ///
    /// // Remove the entry from the IDR.
    /// assert!(idr.remove(key));
    ///
    /// // Repeat removal will return false.
    /// assert!(!idr.remove(key));
    ///
    /// // Now, the entry is unrechable using IDR.
    /// assert!(!idr.contains(key));
    ///
    /// // However, it still can be accessed using the handle.
    /// assert_eq!(entry, "foo");
    ///
    /// // An object behind the entry is not dropped until all handles are dropped.
    /// // However, the real destruction of the object can be delayed according to EBR.
    /// drop(entry);
    /// ```
    #[inline]
    pub fn remove(&self, key: Key) -> bool {
        let page_no = key.page_no::<C>();
        self.pages
            .get(page_no.to_usize())
            .map_or(false, |page| page.remove(key))
    }

    /// Returns a borrowed handle to the entry associated with the given key,
    /// or `None` if the IDR contains no entry for the given key.
    ///
    /// This method is wait-free.
    ///
    /// While the handle exists, it indicates to the IDR that the entry the
    /// handle references is currently being accessed. If the entry is
    /// removed from the IDR while a handle exists, it's still accessible via
    /// the handle.
    ///
    /// This method **doesn't modify memory**, thus it creates no contention on
    /// it at all. This is the whole point of the EBR pattern and the reason
    /// why it's used here.
    ///
    /// The returned handle cannot be send to another thread.
    /// Also, it means it cannot be hold over `.await` points.
    ///
    /// # Example
    /// ```
    /// # use std::num::NonZeroU64;
    /// # use idr_ebr::Idr;
    /// let idr = Idr::default();
    /// let key = idr.insert("foo").unwrap();
    ///
    /// let entry = idr.get(key).unwrap();
    /// assert_eq!(entry, "foo");
    ///
    /// // If the entry is removed, the handle is still valid.
    /// assert!(idr.remove(key));
    /// assert_eq!(entry, "foo");
    ///
    /// // Getting entry for an unknown key produces None.
    /// assert!(idr.get(NonZeroU64::new(12345).unwrap().into()).is_none());
    /// ```
    #[inline]
    pub fn get(&self, key: Key) -> Option<BorrowedEntry<'_, T>> {
        let guard = ebr::Guard::new();
        let page_no = key.page_no::<C>();
        let page = self.pages.get(page_no.to_usize())?;
        let value = page.get(key, &guard).as_ref()?;

        Some(BorrowedEntry {
            value: unsafe { mem::transmute(value) },
            _guard: guard,
        })
    }

    /// Returns a owned handle to the entry associated with the given key,
    /// or `None` if the IDR contains no entry for the given key.
    ///
    /// This method is lock-free.
    ///
    /// While the handle exists, it indicates to the IDR that the entry the
    /// handle references is currently being accessed. If the entry is
    /// removed from the IDR while a handle exists, it's still accessible via
    /// the handle.
    ///
    /// Unlike [`Idr::get()`], which borrows the IDR, this method holds a strong
    /// reference to the object itself:
    /// * It modify the memory and, therefore, creates contention on it.
    /// * The IDR can be dropped while the handle exists.
    /// * It can be send to another thread.
    /// * It can be hold over `.await` points.
    ///
    /// # Example
    /// ```
    /// # use idr_ebr::Idr;
    /// let idr = Idr::default();
    /// let key = idr.insert("foo").unwrap();
    ///
    /// let entry = idr.get_owned(key).unwrap();
    ///
    /// // The IDR can be dropped.
    /// drop(idr);
    ///
    /// // The handle can be send to another thread.
    /// std::thread::spawn(move || {
    ///     assert_eq!(entry, "foo");
    /// }).join().unwrap();
    /// ```
    #[inline]
    pub fn get_owned(&self, key: Key) -> Option<OwnedEntry<T>> {
        let guard = ebr::Guard::new();
        let page_no = key.page_no::<C>();
        let page = self.pages.get(page_no.to_usize())?;
        page.get(key, &guard).get_shared().map(OwnedEntry)
    }

    /// Returns `true` if the IDR contains an entry for the given key.
    ///
    /// This method is wait-free.
    ///
    /// # Example
    /// ```
    /// # use idr_ebr::Idr;
    /// let idr = Idr::default();
    ///
    /// let key = idr.insert("foo").unwrap();
    /// assert!(idr.contains(key));
    ///
    /// idr.remove(key);
    /// assert!(!idr.contains(key));
    /// ```
    #[inline]
    pub fn contains(&self, key: Key) -> bool {
        self.get(key).is_some()
    }
}

// === VacantEntry ===

/// A handle to a vacant entry in an IDR.
///
/// It allows constructing values with the key that they will be assigned to.
///
/// See [`Idr::vacant_entry()`] for more details.
#[must_use]
pub struct VacantEntry<'s, T: 'static, C: Config> {
    page: &'s Page<T, C>,
    slot: &'s Slot<T, C>,
    key: Key,
}

impl<T: 'static, C: Config> VacantEntry<'_, T, C> {
    /// Returns the key at which this entry will be inserted.
    ///
    /// An entry stored in this entry will be associated with this key.
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
        self.page.add_free(self.slot);
    }
}

// === BorrowedEntry ===

/// A borrowed handle that allows access to an occupied entry in an IDR.
///
/// See [`Idr::get()`] for more details.
#[must_use]
pub struct BorrowedEntry<'s, T> {
    value: &'s T,
    _guard: ebr::Guard,
}

impl<T> Deref for BorrowedEntry<'_, T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.value
    }
}

impl<T: fmt::Debug> fmt::Debug for BorrowedEntry<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.value, f)
    }
}

impl<T: PartialEq<T>> PartialEq<T> for BorrowedEntry<'_, T> {
    fn eq(&self, other: &T) -> bool {
        self.value.eq(other)
    }
}

// === OwnedEntry ===

/// An owned handle that allows access to an occupied entry in an IDR.
///
/// See [`Idr::get_owned()`] for more details.
#[must_use]
pub struct OwnedEntry<T>(ebr::Shared<T>);

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
    fn eq(&self, other: &T) -> bool {
        self.0.eq(other)
    }
}

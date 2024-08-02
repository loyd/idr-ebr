#![doc = include_str!("../README.md")]

use std::fmt;

use self::{config::ConfigPrivate, control::PageControl, key::PageNo, page::Page};

mod config;
mod control;
mod handles;
mod key;
mod loom;
mod page;
mod slot;

pub use self::{
    config::{Config, DefaultConfig},
    handles::{BorrowedEntry, Iter, OwnedEntry, VacantEntry},
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
    page_control: PageControl,
}

impl<T: 'static> Default for Idr<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: 'static, C: Config> Idr<T, C> {
    /// The number of bits in each key which are used by the IDR.
    ///
    /// If other data is packed into the keys returned by [`Idr::insert()`],
    /// user code is free to use any bits higher than the `USED_BITS`-th bit.
    ///
    /// This is determined by the [`Config`] type that configures the IDR's
    /// parameters. By default, all bits are used; this can be changed by
    /// overriding the [`Config::RESERVED_BITS`] constant.
    pub const USED_BITS: u32 = C::USED_BITS;

    /// Returns a new IDR with the provided configuration parameters.
    pub fn new() -> Self {
        // Perform compile-time postmono checks.
        assert!(C::ENSURE_VALID);

        Self {
            pages: (0..C::MAX_PAGES).map(PageNo::new).map(Page::new).collect(),
            page_control: PageControl::default(),
        }
    }

    /// Inserts a value into the IDR, returning the key at which that
    /// value was inserted. This key can then be used to access the entry.
    ///
    /// This method is, usually, lock-free. However, it can block if a new page
    /// should be allocated. Thus, it can block max [`Config::MAX_PAGES`] times.
    /// Once allocated, the page is never deallocated until the IDR is dropped.
    ///
    /// Returns `None` if there is no more space in the IDR,
    /// and no items can be added until some are removed.
    ///
    /// # Panics
    ///
    /// If a new page should be allocated, but the allocator fails.
    ///
    /// # Example
    ///
    /// ```
    /// use idr_ebr::{Idr, EbrGuard};
    ///
    /// let idr = Idr::default();
    /// let key = idr.insert("foo").unwrap();
    /// assert_eq!(idr.get(key, &EbrGuard::new()).unwrap(), "foo");
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
    /// should be allocated. Thus, it can block max [`Config::MAX_PAGES`] times.
    /// Once allocated, the page is never deallocated until the IDR is dropped.
    ///
    /// This method is useful when creating values that must contain their
    /// IDR key. The returned [`VacantEntry`] reserves a slot in the IDR and
    /// is able to return the key of the entry.
    ///
    /// Returns `None` if there is no more space in the IDR,
    /// and no items can be added until some are removed.
    ///
    /// # Panics
    ///
    /// If a new page should be allocated, but the allocator fails.
    ///
    /// # Example
    ///
    /// ```
    /// use idr_ebr::{Idr, EbrGuard};
    ///
    /// let idr = Idr::default();
    ///
    /// let key = {
    ///     let entry = idr.vacant_entry().unwrap();
    ///     let key = entry.key();
    ///     entry.insert((key, "foo"));
    ///     key
    /// };
    ///
    /// assert_eq!(idr.get(key, &EbrGuard::new()).unwrap().0, key);
    /// assert_eq!(idr.get(key, &EbrGuard::new()).unwrap().1, "foo");
    /// ```
    #[inline]
    pub fn vacant_entry(&self) -> Option<VacantEntry<'_, T, C>> {
        self.page_control.choose(&self.pages, |page| {
            page.reserve(&self.page_control)
                .map(|(key, slot)| VacantEntry::new(page, slot, key))
        })
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
    ///
    /// ```
    /// use idr_ebr::{Idr, EbrGuard};
    ///
    /// let idr = Idr::default();
    /// let key = idr.insert("foo").unwrap();
    ///
    /// let guard = EbrGuard::new();
    /// let entry = idr.get(key, &guard).unwrap();
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
    /// // The real destruction of the object can be delayed according to EBR.
    /// drop(guard);
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
    ///
    /// ```
    /// use idr_ebr::{Idr, EbrGuard, Key};
    ///
    /// let idr = Idr::default();
    /// let key = idr.insert("foo").unwrap();
    ///
    /// let guard = EbrGuard::new();
    /// let entry = idr.get(key, &guard).unwrap();
    /// assert_eq!(entry, "foo");
    ///
    /// // If the entry is removed, the handle is still valid.
    /// assert!(idr.remove(key));
    /// assert_eq!(entry, "foo");
    ///
    /// // Getting entry for an unknown key produces None.
    /// assert!(idr.get(Key::try_from(12345).unwrap(), &guard).is_none());
    /// ```
    #[inline]
    pub fn get<'g>(&self, key: Key, guard: &'g EbrGuard) -> Option<BorrowedEntry<'g, T>> {
        let page_no = key.page_no::<C>();
        let page = self.pages.get(page_no.to_usize())?;
        page.get(key, guard)
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
    ///
    /// ```
    /// use idr_ebr::Idr;
    ///
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
        self.get(key, &EbrGuard::new())?.to_owned()
    }

    /// Returns `true` if the IDR contains an entry for the given key.
    ///
    /// This method is wait-free.
    ///
    /// # Example
    ///
    /// ```
    /// use idr_ebr::Idr;
    ///
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
        self.get(key, &EbrGuard::new()).is_some()
    }

    /// Returns a fused iterator over all occupied entries in the IDR.
    /// An order of iteration is not guaranteed. Added during iteration entries
    /// can be observed via the iterator, but it depends on the current position
    /// of the iterator.
    ///
    /// This method is wait-free and [`Iter::next()`] is also wait-free.
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
    /// The returned iterator cannot be send to another thread.
    /// Also, it means it cannot be hold over `.await` points.
    ///
    /// # Example
    ///
    /// ```
    /// use idr_ebr::{Idr, EbrGuard};
    ///
    /// let idr = Idr::default();
    /// let foo_key = idr.insert("foo").unwrap();
    /// let bar_key = idr.insert("bar").unwrap();
    ///
    /// let guard = EbrGuard::new();
    /// let mut iter = idr.iter(&guard);
    ///
    /// let (key, entry) = iter.next().unwrap();
    /// assert_eq!(key, foo_key);
    /// assert_eq!(entry, "foo");
    ///
    /// let (key, entry) = iter.next().unwrap();
    /// assert_eq!(key, bar_key);
    /// assert_eq!(entry, "bar");
    ///
    /// let baz_key = idr.insert("baz").unwrap();
    /// let (key, entry) = iter.next().unwrap();
    /// assert_eq!(key, baz_key);
    /// assert_eq!(entry, "baz");
    /// ```
    #[inline]
    pub fn iter<'g>(&self, guard: &'g EbrGuard) -> Iter<'g, '_, T, C> {
        Iter::new(&self.pages, guard)
    }
}

impl<T, C: Config> fmt::Debug for Idr<T, C> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Idr")
            .field("allocated_pages", &self.page_control.allocated())
            .field("config", &C::debug())
            .finish_non_exhaustive()
    }
}

// === EbrGuard ===

/// [`EbrGuard`] allows to access entries of [`Idr`].
///
/// Wraps [`sdd::Guard`] in order to avoid potential breaking changes.
#[derive(Default)]
#[must_use]
pub struct EbrGuard(sdd::Guard);

impl EbrGuard {
    /// Creates a new [`EbrGuard`].
    ///
    /// # Panics
    ///
    /// The maximum number of [`EbrGuard`] instances in a thread is limited to
    /// `u32::MAX`; a thread panics when the number of [`EbrGuard`]
    /// instances in the thread exceeds the limit.
    ///
    /// # Examples
    ///
    /// ```
    /// use idr_ebr::EbrGuard;
    ///
    /// let guard = EbrGuard::new();
    /// ```
    #[inline]
    pub fn new() -> Self {
        Self(sdd::Guard::new())
    }
}

impl fmt::Debug for EbrGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EbrGuard").finish()
    }
}

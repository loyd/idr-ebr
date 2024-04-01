use std::{fmt::Debug, marker::PhantomData};

/// Configuration parameters to tune the behavior of an IDR.
///
/// The capacity of an IDR is determined by the configuration parameters:
/// ```text
///     (2**MAX_PAGES - 1) * INITIAL_PAGE_SIZE
/// ```
///
/// [`Idr::new()`] checks that the configuration is valid at compile time.
/// These checks are triggered by `cargo build` or `cargo test`, but not
/// by `cargo check`.
///
/// [`Idr::new()`]: crate::Idr::new
pub trait Config: Sized {
    /// The capacity of the first page.
    ///
    /// When a page in an underlying slab has been filled with values, a new
    /// page will be allocated that is twice as large as the previous page.
    /// Thus, the second page will be twice this size, and the third will be
    /// four times this size, and so on.
    ///
    /// **Must** be a power of two.
    const INITIAL_PAGE_SIZE: u32 = DefaultConfig::INITIAL_PAGE_SIZE;

    /// The maximum number of pages in an underlying slab of an IDR.
    ///
    /// This value, in combination with `INITIAL_PAGE_SIZE`, determines how many
    /// bits of each key are used to represent slot indices.
    ///
    /// **Must** be positive.
    const MAX_PAGES: u32 = DefaultConfig::MAX_PAGES;

    /// A number of **high-order** bits which are reserved from user code.
    ///
    /// Note: these bits are taken from the generation counter; reserving
    /// additional bits will decrease the period of the generation counter.
    /// These should thus be used relatively sparingly, to ensure that
    /// generation counters are able to effectively prevent the ABA problem.
    ///
    /// **Must** be less than or equal to 32.
    const RESERVED_BITS: u32 = DefaultConfig::RESERVED_BITS;

    /// Returns a debug representation of the configuration, which includes all
    /// internally calculated values and limits.
    #[must_use]
    fn debug() -> impl Debug {
        DebugConfig::<Self>(PhantomData)
    }
}

/// A default configuration:
/// * No bits reserved for user code.
/// * A capacity is 4,294,967,264.
/// * A generation counter with a period of 4,294,967,296.
#[allow(missing_debug_implementations)] // `Config::debug()` instead
pub struct DefaultConfig;

impl Config for DefaultConfig {
    const INITIAL_PAGE_SIZE: u32 = 32;
    const MAX_PAGES: u32 = 27;
    const RESERVED_BITS: u32 = 0;
}

pub(crate) trait ConfigPrivate: Config {
    const USED_BITS: u32 = 64 - Self::RESERVED_BITS;
    const SLOT_BITS: u32 = Self::MAX_PAGES + Self::INITIAL_PAGE_SIZE.trailing_zeros();
    const SLOT_MASK: u32 = ((1u64 << Self::SLOT_BITS) - 1) as u32;
    const GENERATION_BITS: u32 = Self::USED_BITS - Self::SLOT_BITS;
    const GENERATION_MASK: u32 = ((1u64 << Self::GENERATION_BITS) - 1) as u32;

    // For debugging and tests, both values are `<= u32::MAX + 1`.
    const MAX_SLOTS: u64 = ((1u64 << Self::MAX_PAGES) - 1) * Self::INITIAL_PAGE_SIZE as u64;
    const MAX_GENERATIONS: u64 = 1u64 << Self::GENERATION_BITS;

    // Compile-time (only test/build, not check) postmono constraints.
    // https://t.me/c/1601845432/304
    const ENSURE_VALID: bool = {
        assert!(Self::INITIAL_PAGE_SIZE.is_power_of_two());
        assert!(Self::MAX_PAGES > 0);
        assert!(Self::RESERVED_BITS <= 32);
        assert!(Self::SLOT_BITS <= 32);
        assert!(Self::GENERATION_BITS <= 32);
        true
    };
}

impl<C: Config> ConfigPrivate for C {}

struct DebugConfig<C>(PhantomData<C>);

impl<C: Config> Debug for DebugConfig<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(std::any::type_name::<C>())
            .field("INITIAL_PAGE_SIZE", &C::INITIAL_PAGE_SIZE)
            .field("MAX_PAGES", &C::MAX_PAGES)
            .field("RESERVED_BITS", &C::RESERVED_BITS)
            .field("USED_BITS", &C::USED_BITS)
            .field("SLOT_BITS", &C::SLOT_BITS)
            .field("GENERATION_BITS", &C::GENERATION_BITS)
            .field("MAX_SLOTS", &C::MAX_SLOTS)
            .field("MAX_GENERATIONS", &C::MAX_GENERATIONS)
            .finish()
    }
}

#[test]
fn test_default_config() {
    assert_eq!(DefaultConfig::USED_BITS, 64);
    assert_eq!(DefaultConfig::SLOT_BITS, 32);
    assert_eq!(DefaultConfig::SLOT_MASK, u32::MAX);
    assert_eq!(DefaultConfig::GENERATION_BITS, 32);
    assert_eq!(DefaultConfig::GENERATION_MASK, u32::MAX);
    assert_eq!(DefaultConfig::MAX_SLOTS, 4_294_967_264);
    assert_eq!(DefaultConfig::MAX_GENERATIONS, 4_294_967_296);
}

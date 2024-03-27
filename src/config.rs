use std::{fmt::Debug, marker::PhantomData};

/// TODO
pub trait Config {
    const INITIAL_PAGE_SIZE: u32;
    const MAX_PAGES: u32;
    const RESERVED_BITS: u32;

    fn debug() -> impl Debug
    where
        Self: Sized,
    {
        DebugConfig::<Self>(PhantomData)
    }
}

pub struct DefaultConfig;

impl Config for DefaultConfig {
    // TODO: compile-time check for power of 2
    const INITIAL_PAGE_SIZE: u32 = 32;
    const MAX_PAGES: u32 = 27;
    const RESERVED_BITS: u32 = 0;
}

// TODO check CONSTRAINTS:
// SLOT_BITS <= 32
// GENERATION_BITS <= 32
// RESERVED_BITS <= 32
// SLOT_BITS + GENERATION_BITS = USED_BITS
// USED_BITS + RESERVED_BITS = 64

pub(crate) trait ConfigPrivate: Config {
    const USED_BITS: u32 = 64 - Self::RESERVED_BITS;
    const SLOT_BITS: u32 = Self::MAX_PAGES + Self::INITIAL_PAGE_SIZE.trailing_zeros();
    const SLOT_MASK: u32 = ((1u64 << Self::SLOT_BITS) - 1) as u32;
    const GENERATION_BITS: u32 = Self::USED_BITS - Self::SLOT_BITS;
    const GENERATION_MASK: u32 = ((1u64 << Self::GENERATION_BITS) - 1) as u32;
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
            .field(
                "MAX_SLOTS",
                &((2u64.pow(C::MAX_PAGES) - 1) * u64::from(C::INITIAL_PAGE_SIZE)),
            )
            .field("MAX_GENERATIONS", &2u64.pow(C::GENERATION_BITS))
            .finish()
    }
}

#[test]
fn test_default_config() {
    assert_eq!(DefaultConfig::USED_BITS, 64);
    assert_eq!(DefaultConfig::SLOT_BITS, 32);
    assert_eq!(DefaultConfig::SLOT_MASK, u32::MAX);
    assert_eq!(DefaultConfig::GENERATION_BITS, 32);
}

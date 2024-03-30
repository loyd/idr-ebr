use idr_ebr::{Config, Idr};

#[test]
fn default() {
    let _: Idr<u64> = <_>::default();
}

#[test]
fn reserved() {
    struct CustomConfig<const R: u32>;

    impl<const R: u32> Config for CustomConfig<R> {
        const INITIAL_PAGE_SIZE: u32 = 32;
        const MAX_PAGES: u32 = 27;
        const RESERVED_BITS: u32 = R;
    }

    let _ = Idr::<u64, CustomConfig<0>>::new();
    let _ = Idr::<u64, CustomConfig<1>>::new();
    let _ = Idr::<u64, CustomConfig<10>>::new();
    let _ = Idr::<u64, CustomConfig<32>>::new();
}

#[test]
fn invalid() {
    let t = trybuild::TestCases::new();
    // WA for https://github.com/dtolnay/trybuild/issues/258
    t.pass("tests/config/_force_build.rs");

    // Cases.
    t.compile_fail("tests/config/ips_not_power_of_two.rs");
    t.compile_fail("tests/config/max_pages_zero.rs");
    t.compile_fail("tests/config/reserved_bits_too_big.rs");
    t.compile_fail("tests/config/slot_bits_too_big.rs");
    t.compile_fail("tests/config/generation_bits_too_big.rs");
}

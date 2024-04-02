use idr_ebr::{Config, Guard, Idr};

#[test]
fn few_slots() {
    struct FewSlotsConfig;
    impl Config for FewSlotsConfig {
        const INITIAL_PAGE_SIZE: u32 = 1;
        const MAX_PAGES: u32 = 4;
        const RESERVED_BITS: u32 = 32;
    }

    let idr = Idr::<u64, FewSlotsConfig>::new();

    for _ in 0..3 {
        let keys = (0..15)
            .map(|i| (idr.insert(i).unwrap(), i))
            .collect::<Vec<_>>();

        for &(key, value) in &keys {
            assert_eq!(idr.get(key, &Guard::new()).unwrap(), value);
        }

        assert!(idr.insert(0).is_none());

        // Remove everything.
        for (key, _) in keys {
            assert!(idr.remove(key));
        }
    }
}

#[test]
fn zero_generations() {
    struct ZeroGenerationsConfig;
    impl Config for ZeroGenerationsConfig {
        const RESERVED_BITS: u32 = 32;
    }

    let idr = Idr::<u64, ZeroGenerationsConfig>::new();

    let key = idr.insert(0).unwrap();
    assert!(idr.remove(key));

    let key2 = idr.insert(0).unwrap();
    assert_eq!(key2, key);
}

#[test]
fn few_generations() {
    struct FewGenerationsConfig;
    impl Config for FewGenerationsConfig {
        const MAX_PAGES: u32 = 26;
        const RESERVED_BITS: u32 = 32;
    }

    let idr = Idr::<u64, FewGenerationsConfig>::new();

    let key = idr.insert(0).unwrap(); // generation=0
    assert!(idr.remove(key));

    let key2 = idr.insert(0).unwrap(); // generation=1
    assert_ne!(key2, key);
    assert!(idr.remove(key2));

    let key3 = idr.insert(0).unwrap(); // generation=0
    assert_ne!(key3, key2);
    assert_eq!(key3, key);
}

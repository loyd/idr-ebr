use idr_ebr::{Config, Guard, Idr, Key};

#[test]
fn smoke() {
    let idr: Idr<_> = Idr::default();

    // Insert values.
    let mut keys = (0..100)
        .map(|i| (idr.insert(i).unwrap(), i))
        .collect::<Vec<_>>();

    // Check that the values are accessible.
    for (key, value) in &keys {
        assert_eq!(&*idr.get(*key, &Guard::new()).unwrap(), value);
    }

    check_iter(&idr, keys.clone());

    // Remove every other key.
    for (key, value) in &keys {
        if value % 2 == 0 {
            assert!(idr.remove(*key));
        }
    }

    // Check that remaining values are accessible.
    keys.retain(|(key, value)| {
        let guard = Guard::new();
        let actual = idr.get(*key, &guard);

        if value % 2 == 0 {
            assert!(actual.is_none());
            false
        } else {
            assert_eq!(actual.unwrap(), *value);
            true
        }
    });

    check_iter(&idr, keys);

    fn check_iter(idr: &Idr<i32>, mut expected: Vec<(Key, i32)>) {
        let mut actual = idr
            .iter(&Guard::new())
            .map(|(key, entry)| (key, *entry))
            .collect::<Vec<_>>();

        actual.sort();
        expected.sort();

        assert_eq!(actual, expected);
    }
}

#[test]
fn extension() {
    struct TinyConfig;
    impl Config for TinyConfig {
        const INITIAL_PAGE_SIZE: u32 = 4;
        const MAX_PAGES: u32 = 5;
        const RESERVED_BITS: u32 = 32;
    }

    let idr: Idr<_, TinyConfig> = Idr::new();
    assert_pages(&idr, 0);

    for expected_pages in 1..=TinyConfig::MAX_PAGES {
        let capacity = ((1 << (expected_pages - 1)) - 1) * TinyConfig::INITIAL_PAGE_SIZE;

        for i in 0..(2 << expected_pages) {
            idr.insert(capacity + i).unwrap();
        }
        assert_pages(&idr, expected_pages);

        let len = idr
            .iter(&Guard::new())
            .enumerate()
            .inspect(|(i, (_, value))| assert_eq!(*value, u32::try_from(*i).unwrap()))
            .count();

        assert_eq!(
            u32::try_from(len).unwrap(),
            capacity + (2 << expected_pages)
        );
    }

    assert!(idr.insert(42).is_none());

    fn assert_pages(idr: impl std::fmt::Debug, expected: u32) {
        assert!(format!("{idr:?}").contains(&format!("allocated_pages: {expected}")));
    }
}

#[test]
fn reuse() {
    struct TinyConfig;
    impl Config for TinyConfig {
        const INITIAL_PAGE_SIZE: u32 = 4;
        const MAX_PAGES: u32 = 29;
        const RESERVED_BITS: u32 = 32;
    }

    assert!(format!("{:?}", TinyConfig::debug()).contains("GENERATION_BITS: 1"));

    let idr: Idr<_, TinyConfig> = Idr::new();

    let key = idr.insert(0).unwrap();
    assert!(idr.remove(key));

    let key2 = idr.insert(1).unwrap();
    assert!(!idr.contains(key));
    assert!(idr.remove(key2));

    let key3 = idr.insert(2).unwrap();
    assert_eq!(key3, key);
    assert_eq!(idr.get(key, &Guard::new()).unwrap(), 2);
    assert!(idr.remove(key));
    assert!(!idr.contains(key3));
}

#[test]
fn invalid_key() {
    let idr = Idr::<i32>::default();
    let invalid_key = Key::try_from(1).unwrap();

    // Shouldn't panic.
    idr.get(invalid_key, &Guard::new());
}

use idr_ebr::{Guard, Idr, Key};

#[test]
fn it_works() {
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
}

fn check_iter(idr: &Idr<i32>, mut expected: Vec<(Key, i32)>) {
    let mut actual = idr
        .iter(&Guard::new())
        .map(|(key, entry)| (key, *entry))
        .collect::<Vec<_>>();

    actual.sort();
    expected.sort();

    assert_eq!(actual, expected);
}

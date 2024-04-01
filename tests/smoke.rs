use idr_ebr::Idr;

#[test]
fn it_works() {
    let idr: Idr<_> = Idr::default();

    // Insert values.
    let keys = (0..100)
        .map(|i| (i, idr.insert(i).unwrap()))
        .collect::<Vec<_>>();

    // Check that the values are accessible.
    for (value, key) in &keys {
        assert_eq!(&*idr.get(*key).unwrap(), value);
    }

    // Remove every other key.
    for (value, key) in &keys {
        if value % 2 == 0 {
            assert!(idr.remove(*key));
        }
    }

    // Check that remaining values are accessible.
    for (value, key) in &keys {
        let actual = idr.get(*key);

        if value % 2 == 0 {
            assert!(actual.is_none());
        } else {
            assert_eq!(actual.unwrap(), *value);
        }
    }
}

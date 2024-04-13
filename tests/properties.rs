//! This module contains property-based tests against the public API:
//! * API never panics.
//! * Active entries cannot be overridden until removed.
//! * The IDR doesn't produce overlapping keys.
//! * The IDR doesn't leave "lost" keys.
//! * `get()`, `get_owned`, and `contains()` are consistent.
//! * `RESERVED_BITS` are actually not used.
//!
//! The test is supposed to be deterministic.
//!
//! We're not checking concurrency issues here, they should be covered by loom
//! tests anyway. Thus, it's fine to run all actions consequently.

use std::ops::Range;

use indexmap::IndexMap;
use proptest::prelude::*;

use idr_ebr::{Config, DefaultConfig, Guard, Idr, Key};

const ACTIONS: Range<usize> = 1..1000;

#[derive(Debug, Clone)]
enum Action {
    Insert,
    VacantEntry,
    RemoveRandom(Key),
    RemoveExistent(/* seed */ usize),
    GetRandom(Key),
    GetExistent(/* seed */ usize),
}

fn action_strategy() -> impl Strategy<Value = Action> {
    prop_oneof![
        1 => Just(Action::Insert),
        1 => Just(Action::VacantEntry),
        1 => key_strategy().prop_map(Action::RemoveRandom),
        1 => prop::num::usize::ANY.prop_map(Action::RemoveExistent),
        // Produce `GetRandom` and `GetExistent` more often.
        5 => key_strategy().prop_map(Action::GetRandom),
        5 => prop::num::usize::ANY.prop_map(Action::GetExistent),
    ]
}

fn key_strategy() -> impl Strategy<Value = Key> {
    (1u64..u64::from(u16::MAX)).prop_map(|v| Key::try_from(v).unwrap())
}

/// Stores active entries (added and not yet removed).
#[derive(Default)]
struct Active {
    // Use `IndexMap` to preserve determinism.
    map: IndexMap<Key, u32>,
    prev_value: u32,
}

impl Active {
    fn next_value(&mut self) -> u32 {
        self.prev_value += 1;
        self.prev_value
    }

    fn get(&self, key: Key) -> Option<u32> {
        self.map.get(&key).copied()
    }

    fn get_any(&self, seed: usize) -> Option<(Key, u32)> {
        if self.map.is_empty() {
            return None;
        }

        let index = seed % self.map.len();
        self.map.get_index(index).map(|(k, v)| (*k, *v))
    }

    fn insert(&mut self, key: Key, value: u32) {
        assert_eq!(
            self.map.insert(key, value),
            None,
            "keys of active entries must be unique"
        );
    }

    fn remove(&mut self, key: Key) -> Option<u32> {
        self.map.swap_remove(&key)
    }

    fn remove_any(&mut self, seed: usize) -> Option<(Key, u32)> {
        if self.map.is_empty() {
            return None;
        }

        let index = seed % self.map.len();
        self.map.swap_remove_index(index)
    }

    fn drain(&mut self) -> impl Iterator<Item = (Key, u32)> + '_ {
        self.map.drain(..)
    }
}

fn used_bits<C: Config>(key: Key) -> Key {
    assert_eq!(C::RESERVED_BITS + Idr::<u32, C>::USED_BITS, 64);

    let raw_key = u64::from(key);
    let refined = raw_key & ((!0) >> C::RESERVED_BITS);
    Key::try_from(refined).unwrap()
}

#[allow(clippy::needless_pass_by_value)]
fn apply_action<C: Config>(
    idr: &Idr<u32, C>,
    active: &mut Active,
    action: Action,
) -> Result<(), TestCaseError> {
    match action {
        Action::Insert => {
            let value = active.next_value();
            let key = idr.insert(value).expect("unexpectedly exhausted idr");
            prop_assert_eq!(used_bits::<C>(key), key);
            active.insert(key, value);
        }
        Action::VacantEntry => {
            let value = active.next_value();
            let entry = idr.vacant_entry().expect("unexpectedly exhausted idr");
            let key = entry.key();
            prop_assert_eq!(used_bits::<C>(key), key);
            entry.insert(value);
            active.insert(key, value);
        }
        Action::RemoveRandom(key) => {
            let used_key = used_bits::<C>(key);
            prop_assert_eq!(
                idr.get(key, &Guard::new()).map(|e| *e),
                idr.get(used_key, &Guard::new()).map(|e| *e)
            );
            prop_assert_eq!(idr.remove(key), active.remove(used_key).is_some());
        }
        Action::RemoveExistent(seed) => {
            if let Some((key, _value)) = active.remove_any(seed) {
                prop_assert!(idr.contains(key));
                prop_assert!(idr.remove(key));
            }
        }
        Action::GetRandom(key) => {
            let used_key = used_bits::<C>(key);
            prop_assert_eq!(
                idr.get(key, &Guard::new()).map(|e| *e),
                idr.get(used_key, &Guard::new()).map(|e| *e)
            );
            prop_assert_eq!(
                idr.get(key, &Guard::new()).map(|e| *e),
                active.get(used_key)
            );
            prop_assert_eq!(idr.get_owned(key).map(|e| *e), active.get(used_key));
        }
        Action::GetExistent(seed) => {
            if let Some((key, value)) = active.get_any(seed) {
                prop_assert!(idr.contains(key));
                prop_assert_eq!(idr.get(key, &Guard::new()).map(|e| *e), Some(value));
                prop_assert_eq!(idr.get_owned(key).map(|e| *e), Some(value));
            }
        }
    }

    Ok(())
}

fn run<C: Config>(actions: Vec<Action>) -> Result<(), TestCaseError> {
    let idr = Idr::<u32, C>::new();
    let mut active = Active::default();

    // Apply all actions.
    for action in actions {
        apply_action::<C>(&idr, &mut active, action)?;
    }

    // Ensure the IDR contains all remaining entries.
    let mut expected_values = Vec::new();
    for (key, value) in active.drain() {
        prop_assert!(idr.contains(key));
        prop_assert_eq!(idr.get(key, &Guard::new()).map(|e| *e), Some(value));
        prop_assert_eq!(idr.get_owned(key).map(|e| *e), Some(value));
        expected_values.push(value);
    }
    expected_values.sort_unstable();

    // Ensure `iter()` returns all remaining entries.
    let mut actual_values = idr.iter(&Guard::new()).map(|(_, v)| *v).collect::<Vec<_>>();
    actual_values.sort_unstable();
    prop_assert_eq!(actual_values, expected_values);

    Ok(())
}

proptest! {
    #[test]
    fn default_config(actions in prop::collection::vec(action_strategy(), ACTIONS)) {
        run::<DefaultConfig>(actions)?;
    }

    #[test]
    fn medium_config(actions in prop::collection::vec(action_strategy(), ACTIONS)) {
        run::<MediumConfig>(actions)?;
    }

    #[test]
    fn tiny_config(actions in prop::collection::vec(action_strategy(), ACTIONS)) {
        run::<TinyConfig>(actions)?;
    }
}

struct MediumConfig;
impl Config for MediumConfig {
    const MAX_PAGES: u32 = 20;
    const RESERVED_BITS: u32 = 24;
}

struct TinyConfig;
impl Config for TinyConfig {
    const INITIAL_PAGE_SIZE: u32 = 4;
    const RESERVED_BITS: u32 = 3;
}

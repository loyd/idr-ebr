# An efficient concurrent ID to object resolver

[![Crates.io][crates-badge]][crates-url]
[![Documentation][docs-badge]][docs-url]
[![MIT licensed][mit-badge]][mit-url]
[![Build Status][actions-badge]][actions-url]

[crates-badge]: https://img.shields.io/crates/v/idr-ebr.svg
[crates-url]: https://crates.io/crates/idr-ebr
[docs-badge]: https://img.shields.io/docsrs/idr-ebr
[docs-url]: https://docs.rs/idr-ebr
[mit-badge]: https://img.shields.io/badge/license-MIT-blue.svg
[mit-url]: https://github.com/loyd/idr-ebr/blob/master/LICENSE
[actions-badge]: https://github.com/loyd/idr-ebr/actions/workflows/ci.yml/badge.svg
[actions-url]: https://github.com/loyd/idr-ebr/actions/workflows/ci.yml

An IDR (IDentifier Resolver) provides a way to efficiently and concurrently
map integer IDs to references to objects. It's particularly useful in
scenarios where you need to quickly find objects based on their ID:
* IDs are shared among multiple machines or stored on FS
* IDs used for FFI
* IDs are used as cheap replacement for `Weak` smart pointers

The main goal of this crate is to provide a structure for fast getting objects by their IDs.
The most popular solution for this problem is concurrent slabs.
However, an interesting problem concurrent collections deal with comes from the remove operation.
Suppose that a thread removes an element from some lock-free slab, while another thread is reading
that same element at the same time. The first thread must wait until the second thread stops
reading the element. Only then it is safe to destruct it. Thus, every read operation should
actually modify memory in order to tell other threads that the item is accessed right now.
It can lead to high contention and, hence, a huge performance penalty (see benchmarks below)
even if many threads mostly read and don't change data.

The modern solution for this problem is [EBR] (Epoch-Based memory Reclamation).
This crate based on EBR of the [`scc`] crate rather than [`crossbeam-epoch`], because it's more efficient.

Every insertion allocates a new EBR container. Thus, it's preferable to use a strong modern allocator (e.g. [`mimalloc`]) if insertions are frequent.

Note: this crate isn't optimized for insert/remove operations (although it could be), if it's your case,
check [`sharded-slab`], it's the efficient and well-tested implementation of a concurrent slab.

[EBR]: https://stackoverflow.com/a/77647126
[`scc`]: https://crates.io/crates/scc
[`crossbeam-epoch`]: https://crates.io/crates/crossbeam-epoch
[`sharded-slab`]: https://crates.io/crates/sharded-slab
[`mimalloc`]: https://crates.io/crates/mimalloc

## Examples

Inserting an item into the IDR, returning a key:
```rust
use idr_ebr::{Idr, Guard};

let idr = Idr::default();
let key = idr.insert("foo").unwrap();

let guard = Guard::new(); // take EBR guard
assert_eq!(idr.get(key, &guard).unwrap(), "foo");
```

## Safety and Correctness

Most implementations of wait-free and lock-free data structures in Rust require
some amount of unsafe code, and this crate is not an exception.

Therefore, testing should be as complete as possible, which is why many tools are used to verify correctness:
* [`loom`] to check concurrency according to C11 memory model
* [`miri`] to check for the absence of undefined behavior
* [`proptest`] to check some common properties of the implementation

In order to guard against the [ABA] problem, this crate makes use of
_generational indices_. Each slot in the underlying slab tracks a generation
counter which is incremented every time a value is removed from that slot,
and the keys returned by `Idr::insert()` include the generation of the slot
when the value was inserted, packed into the high-order bits of the key.
This ensures that if a value is inserted, removed, and a new value is inserted
into the same slot in the underlying slab, the key returned by the first call
to `insert` will not map to the new value.

Since a fixed number of bits are set aside to use for storing the generation
counter, the counter will wrap around after being incremented a number of
times. To avoid situations where a returned index lives long enough to see the
generation counter wrap around to the same value, it is good to be fairly
generous when configuring the allocation of key bits.

[`loom`]: https://crates.io/crates/loom
[`miri`]: https://github.com/rust-lang/miri
[`proptest`]: https://proptest-rs.github.io/proptest/intro.html
[ABA]: https://en.wikipedia.org/wiki/ABA_problem

## Performance

These graphs were produced by [benchmarks] using the [`criterion`] crate.

The first shows the results of the `read_only` benchmark where an increasing
number of threads accessing the same slot, that leads to high contention.
It compares the performance of the IDR with [`sharded-slab`] and simple `std::sync::Weak::upgrade()`:

TODO

* `idr-pin-once`: one `Guard::new()` for all accesses
* `idr-repin`: new `Guard::new()` on every access
* `weak`: `std::sync::Weak::upgrade()`
* `sharded-slab`: `Slab` with default parameters in the [`sharded-slab`] crate

This benchmark demonstrate that the IDR doesn't create any contention on `get()` at all.

The second graph shows the results of the `insert_remove` benchmark where an increasing
number of threads insert and remove entries from the IDR. As mentioned before, it's not the goal
of this crate and not optimized yet for this reason.

TODO

* `idr`: the `IDR` structure from this crate
* `sharded-slab`: `Slab` from the [`sharded-slab`] crate

[benchmarks]: https://github.com/loyd/idr-ebr/blob/master/benches/contention.rs
[`criterion`]: https://crates.io/crates/criterion

## Implementation

The IDR is based on a slab, where every slot contain a link to EBR container.
Thus, every `Idr::insert()` calls an allocator to create that container.

The container can be used by multiple threads both in temporary way (`Idr::get()` or `Idr::iter()`)
and permanently (`Idr::get_owned()`) even when IDR is already dropped or an entry is removed from the IDR.

```text
IDR                 ┌─────────┐
 #──►┌───────────┐  │    ┌────▼─────┐
     │  page 1   │  │  ┌─┤   next   │
     ├───────────┤  │  │ ├──────────┤
     │  page 2   │  │  │ │generation│
     │           │  │  │ ├──────────┤
     │ free head ├──┘  │ │  vacant  │
     ├───────────┤     │ ├──────────┤
     │  page 3   │     │ ├──────────┤
     └───────────┘     │ │   next   │
          ...          │ ├──────────┤          EBR
          ...          │ │generation│       container
     ┌───────────┐     │ ├──────────┤      ┌─────────┐
     │  page n   │     │ │ occupied ├──────► ref cnt │
     └───────────┘     │ ├──────────┤      ├─────────┤
                       │ ├──────────┤      │         │
      (pages are       └─►   next   │      │  data   │
       lazily            ├──────────┤      │         │
       allocated)        │generation│      └─────────┘
                         ├──────────┤
                         │  vacant  │
                         └──────────┘
```

The size of the first page in a shard is always a power of two, and every subsequent page added after the first is twice as large as the page that preceeds it.
```text
           IPS
  page    ◄───►
  ┌───┐   ┌─┬─┐
  │ 0 ├───▶ │ │
  ├───┤   ├─┼─┼─┬─┐        slots
  │ 1 ├───▶ │ │ │ │
  ├───┤   ├─┼─┼─┼─┼─┬─┬─┬─┐
  │ 2 ├───▶ │ │ │ │ │ │ │ │
  ├───┤   ├─┼─┼─┼─┼─┼─┼─┼─┼─┬─┬─┬─┬─┬─┬─┬─┐
  │ 3 ├───▶ │ │ │ │ │ │ │ │ │ │ │ │ │ │ │ │
  └───┘   └─┴─┴─┴─┴─┴─┴─┴─┴─┴─┴─┴─┴─┴─┴─┴─┘
```

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
scenarios where you need to find objects based on their ID quickly:
* IDs are shared among multiple machines or stored on FS
* IDs are used as a cheaper replacement for `Weak` smart pointers
* IDs are used for FFI with untrusted code

The main goal of this crate is to provide a structure for fast getting objects by their IDs.
The most popular solution for this problem is concurrent slabs.
However, an interesting problem concurrent collections deal with comes from the remove operation.
Suppose that a thread removes an element from some lock-free slab while another thread is reading
that same element at the same time. The first thread must wait until the second thread stops
reading the element. Only then is it safe to destroy it. Thus, every read operation should
actually modify memory in order to tell other threads that the item is accessed right now.
It can lead to high contention and, hence, a huge performance penalty (see benchmarks below)
even if many threads mostly read and don't change data.

The modern solution for this problem is [EBR] (Epoch-Based memory Reclamation).
This crate is based on the EBR of the [`scc`] crate rather than [`crossbeam-epoch`] because it's more efficient.

Every insertion allocates a new EBR container. Thus, it's preferable to use a strong modern allocator (e.g. [`mimalloc`]) if insertions are frequent.

Note: this crate isn't optimized for insert/remove operations (although it could be), if it's your case,
check [`sharded-slab`], it's the efficient and well-tested implementation of a concurrent slab.

[EBR]: https://stackoverflow.com/a/77647126
[`scc`]: https://crates.io/crates/scc
[`crossbeam-epoch`]: https://crates.io/crates/crossbeam-epoch
[`sharded-slab`]: https://crates.io/crates/sharded-slab
[`mimalloc`]: https://crates.io/crates/mimalloc

## Examples

Inserting an item into the IDR, and returning a key:
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
counter, which is incremented every time a value is removed from that slot,
and the keys returned by `Idr::insert()` include the generation of the slot
when the value was inserted, packed into the high-order bits of the key.
This ensures that if a value is inserted, removed, and a new value is inserted
into the same slot in the underlying slab, the key returned by the first call
to `insert` will not map to the new value.

Since a fixed number of bits are set aside to use for storing the generation
counter, the counter will wrap around after being incremented a number of
times. To avoid situations where a returned index lives long enough to see the
generation counter wraps around to the same value, it is good to be fairly
generous when configuring the allocation of key bits.

[`loom`]: https://crates.io/crates/loom
[`miri`]: https://github.com/rust-lang/miri
[`proptest`]: https://proptest-rs.github.io/proptest/intro.html
[ABA]: https://en.wikipedia.org/wiki/ABA_problem

## Performance

These graphs were produced by [benchmarks] using the [`criterion`] crate.

The first shows the results of the `read_only` benchmark where an increasing
number of threads accessing the same slot, which leads to high contention.
It compares the performance of the IDR with [`sharded-slab`] and simple `std::sync::Weak::upgrade()`:

![image](https://github.com/loyd/idr-ebr/assets/952180/099c1ef1-8120-460b-9b7b-83c09834dfb4)

* `idr-pin-once`: one `Guard::new()` for all accesses
* `idr-repin`: new `Guard::new()` on every access
* `weak`: `std::sync::Weak::upgrade()`
* `sharded-slab`: `Slab` with default parameters in the [`sharded-slab`] crate

This benchmark demonstrates that the IDR doesn't create any contention on `get()` at all.

The second graph shows the results of the `insert_remove` benchmark where an increasing
number of threads insert and remove entries from the IDR. As mentioned before, it's not the goal
of this crate, and not optimized yet for this reason.

![image](https://github.com/loyd/idr-ebr/assets/952180/429a60ae-f7ee-470f-a9ef-9b62af077b1a)

* `idr`: the `IDR` structure from this crate
* `sharded-slab`: `Slab` from the [`sharded-slab`] crate

[benchmarks]: https://github.com/loyd/idr-ebr/blob/master/benches/contention.rs
[`criterion`]: https://crates.io/crates/criterion

## Implementation

The IDR is based on a slab, where every slot contains a link to the EBR container.
Thus, every `Idr::insert()` calls an allocator to create that container.

The container can be used by multiple threads both in a temporary way (`Idr::get()` or `Idr::iter()`)
and permanently (`Idr::get_owned()`) even when the IDR is already dropped or an entry is removed from the IDR.

The IDR structure:
```text
IDR     pages               slots
 #─►┌───────────┐  ┌───►┌──────────┐
    │  page #0  │  │  ┌►│   next   │
    ├───────────┤  │  │ ├──────────┤
    │  page #1  │  │  │ │generation│
    │   slots  ─┼──┘  │ ├──────────┤
    │ free head ┼──┐  │ │  vacant  │
    ├───────────┤  │  │ ╞══════════╡
    │  page #2  │  │  │ │   next   │
    └───────────┘  │  │ ├──────────┤      EBR-protected
         ...       │  │ │generation│        container
         ...       │  │ ├──────────┤      ┌───────────┐
    ┌───────────┐  │  │ │ occupied ├─────►│  strong   │
    │ page #M-1 │  │  │ ╞══════════╡      │ reference │
    └───────────┘  │  └─┼─  next   │      │  counter  │
                   └───►├──────────┤      ├───────────┤
     (pages are         │generation│      │           │
      lazily            ├──────────┤      │   value   │
      allocated)        │  vacant  │      │           │
                        └──────────┘      └───────────┘
```

The size of the first page in a shard is always a power of two, and every subsequent page added after the first is twice as large as the page that precedes it:
```text
    pages               slots                  capacity
    ┌────┐   ┌─┬─┐
    │ #0 ├───▶ │x│                               1IPS
    ├────┤   ├─┼─┼─┬─┐
    │ #1 ├───▶ │x│x│ │                           2IPS
    ├────┤   ├─┼─┼─┼─┼─┬─┬─┬─┐
    │ #2 ├───▶ │x│ │x│x│x│ │ │                   4IPS
    ├────┤   ├─┼─┼─┼─┼─┼─┼─┼─┼─┬─┬─┬─┬─┬─┬─┬─┐
    │#M-1├───▶ │ │x│ │x│x│x│ │ │x│ │x│x│x│ │ │   8IPS
    └────┘   └─┴─┴─┴─┴─┴─┴─┴─┴─┴─┴─┴─┴─┴─┴─┴─┘
```
where
* `IPS` is `Config::INITIAL_PAGE_SIZE` (32 by default)
* `M` is `Config::MAX_PAGES` (27 by default)
* `x` is occupied slots

The `Key` structure:
```text
             Key Structure (64b)
    ┌──────────┬────────────┬───────────┐
    │ reserved │ generation │ page+slot │
    │   ≤32b   │    ≤32b    │   ≤32b    │
    │  def=0b  │  def=32b   │  def=32b  │
    └──────────┴────────────┴───────────┘
```

Check `Config` documentation for details how to configure these parts.

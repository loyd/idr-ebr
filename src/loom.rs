#[cfg(not(all(test, loom)))]
pub(crate) use std::{alloc, sync};

#[cfg(all(test, loom))]
pub(crate) use loom::{alloc, sync};

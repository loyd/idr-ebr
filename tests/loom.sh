#!/usr/bin/env bash

# Runs loom tests with defaults for loom's configuration values.
#
# The tests are compiled in release mode to improve performance, but debug
# assertions are enabled.
#
# Any arguments to this script are passed to the `cargo test` invocation.

# Useful:
# LOOM_LOG=debug
# LOOM_CHECKPOINT_FILE=target/loom-checkpoint.json

time RUSTFLAGS="${RUSTFLAGS} --cfg idr_ebr_loom -C debug-assertions" \
     LOOM_MAX_PREEMPTIONS="${LOOM_MAX_PREEMPTIONS:-2}" \
     LOOM_CHECKPOINT_INTERVAL="${LOOM_CHECKPOINT_INTERVAL:-1}" \
     LOOM_LOCATION=1 \
     cargo test --release --features loom --test loom "$@"

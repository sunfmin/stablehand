#!/bin/sh
# The gate: what must pass before anything lands.
set -e
cd "$(dirname "$0")"
cargo fmt --all --check
cargo clippy --all-targets --quiet -- -D warnings
# The tests, measured against .slimfit/baseline.json.
slimfit check
# The release build: both arches, universal, the formula.
DIST=target/release-dry packaging/release.sh dry >/dev/null
# Each engine's stand-in answers every case of its contract (truefit.toml);
# the real engines' side runs outside the gate (`truefit contract --real`).
truefit contract >/dev/null || truefit contract

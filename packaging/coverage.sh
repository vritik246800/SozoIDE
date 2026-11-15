#!/usr/bin/env bash
# The share of lines the tests run (TODO 6.1.3, F44): fails under the minimum, which only goes
# up. cargo-llvm-cov lives in target/tools; LLVM_COV and LLVM_PROFDATA may point at the LLVM of
# the toolchain (rustup's llvm-tools-preview, or Homebrew's llvm with Homebrew's Rust).
set -euo pipefail
MINIMUM=88
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
TOOLS="$ROOT/target/tools"
cd "$ROOT"
if [[ ! -x "$TOOLS/bin/cargo-llvm-cov" ]]; then
    cargo install cargo-llvm-cov --version 0.9.1 --locked --root "$TOOLS"
fi
# The tests of time budgets run in the Test jobs: instrumented, on a small runner, their times
# say nothing (E83).
"$TOOLS/bin/cargo-llvm-cov" llvm-cov --bin sozo --summary-only --fail-under-lines "$MINIMUM" \
    -- --skip within_its_budget --skip a_million_rows

#!/usr/bin/env bash
# The licence of every crate in the binary, for About (TODO 6.3.1): one line each,
# "name version licence", from `cargo metadata`. Run after changing Cargo.lock; a test fails
# while the list misses a crate of the lock file.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
cargo metadata --format-version 1 --locked --offline \
    | python3 -I -c '
import json, sys
data = json.load(sys.stdin)
for package in sorted(data["packages"], key=lambda p: (p["name"], p["version"])):
    print(package["name"], package["version"], package.get("license") or "see its repository")
' > assets/licenses.txt
echo "Wrote assets/licenses.txt ($(wc -l < assets/licenses.txt) crates)"

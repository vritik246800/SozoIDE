#!/usr/bin/env bash
# Before a release is published: its tag is the version in Cargo.toml and its commit is on
# main, where every change arrived through a pull request with CI green.
#   bash packaging/check-release.sh v0.9.0 [commit]
set -euo pipefail
cd "$(dirname "$0")/.."

TAG=${1:?usage: check-release.sh vX.Y.Z [commit]}
COMMIT=${2:-HEAD}
fail() { echo "::error::$*"; echo "FAIL $*" >&2; exit 1; }

[[ "$TAG" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]] || fail "$TAG is not a release tag (vX.Y.Z)"
grep -qx "version = \"${TAG#v}\"" Cargo.toml || fail "Tag $TAG does not match the version in Cargo.toml"
git fetch --quiet --no-tags origin main
git merge-base --is-ancestor "$COMMIT" origin/main || fail "Tag $TAG is not a commit of main"
echo "OK  $TAG is version ${TAG#v}, on main"

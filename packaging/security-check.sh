#!/usr/bin/env bash
# The security gate: the same command locally, in the pre-commit hook and in CI.
#   bash packaging/security-check.sh            the whole repository and the dependency audits
#   bash packaging/security-check.sh --staged   only what the next commit adds (fast, offline)
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."
staged=0
[[ ${1:-} == --staged ]] && staged=1
failed=0
fail() { echo "FAIL $*" >&2; failed=1; }

# 1. Files that never belong in the repository: local rules and notes, secrets, databases.
never='^(Docs/|CLAUDE\.md$|AGENTS\.md$|git\.sh$|\.vercel/|\.claude/)|(^|/)\.env(\.|$)|\.(db|sqlite|sqlite3|pem|p12|pfx|keystore)$'
if ((staged)); then
    files=$(git diff --cached --name-only --diff-filter=ACMR)
else
    files=$(git ls-files)
fi
found=$(grep -E "$never" <<<"$files" | grep -vE '(^|/)\.env\.example$' || true)
[[ -z $found ]] || fail "these files must stay out of the repository:"$'\n'"$found"

# 2. Secrets and personal paths. Lock files only hold hashes that look like keys.
keys='BEGIN [A-Z ]*PRIVATE KEY|AKIA[0-9A-Z]{16}|gh[pousr]_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,}|xox[baprs]-[A-Za-z0-9-]{10,}|sk-[A-Za-z0-9_-]{32,}|AIza[0-9A-Za-z_-]{35}'
assigned='(password|passwd|secret|api_key|token)[[:space:]]*[:=][[:space:]]*["'"'"'][^"'"'"' ]{6,}["'"'"']'
paths='/Users/[A-Za-z]|/home/[a-z]|[A-Z]:\\Users\\[A-Za-z]'
# Neutral values the tests and the site captures use on purpose.
allowed='/home/dev|"very-secret"|packaging/security-check\.sh'
if ((staged)); then
    lines=$(git diff --cached -U0 --diff-filter=ACMR -- . ':!Cargo.lock' ':!site/package-lock.json' \
        | grep -E '^\+' | grep -vE '^\+\+\+' || true)
else
    lines=$(git grep -nIE "$keys|$paths" -- . ':!Cargo.lock' ':!site/package-lock.json' || true)
    lines+=$'\n'$(git grep -nIiE "$assigned" -- . ':!Cargo.lock' ':!site/package-lock.json' || true)
fi
found=$({
    grep -E "$keys|$paths" <<<"$lines" || true
    grep -iE "$assigned" <<<"$lines" || true
} | grep -vE "$allowed" | grep -v '^$' | sort -u | cut -c1-200 || true)
[[ -z $found ]] || fail "possible secret or personal path:"$'\n'"$found"

if ((staged)); then
    exit "$failed"
fi

# 3. Workflows: least privilege, and no trigger that runs a fork's code with the repository token.
for workflow in .github/workflows/*.yml; do
    grep -q '^permissions:' "$workflow" || fail "$workflow has no top-level permissions"
    if grep -q 'pull_request_target' "$workflow"; then fail "$workflow uses pull_request_target"; fi
    if [[ $workflow != */release.yml ]] && grep -qE '^[[:space:]]+contents: write' "$workflow"; then
        fail "$workflow writes to the repository: only the release job may"
    fi
done

# 4. Known vulnerabilities in the dependencies. CI must have the tools; locally a missing one is said.
missing() {
    if [[ ${CI:-} == true ]]; then fail "$1 is not installed"; else echo "SKIP $1 is not installed: $2"; fi
}
audit=target/tools/bin/cargo-audit
[[ -x $audit ]] || audit=$(command -v cargo-audit || true)
if [[ -n $audit ]]; then
    "$audit" audit --quiet || fail "cargo audit found vulnerable crates"
else
    missing cargo-audit 'cargo install --root target/tools --locked cargo-audit'
fi
if command -v npm >/dev/null; then
    (cd site && npm audit --audit-level=high) || fail "npm audit found vulnerable packages in site/"
else
    missing npm 'install Node.js'
fi

[[ $failed == 0 ]] && echo "OK  security check"
exit "$failed"

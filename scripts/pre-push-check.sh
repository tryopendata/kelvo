#!/usr/bin/env bash
# Preserve live evidence even while pre-commit buffers the hook's output.
set -euo pipefail

cd "$(git rev-parse --show-toplevel)"
mkdir -p target/qa/pre-push
evidence="$(mktemp -d "$PWD/target/qa/pre-push/$(date -u +%Y%m%dT%H%M%SZ)-XXXXXX")"
printf 'Pre-push evidence: %s\n' "$evidence"
trap 'printf "%s\n" "$?" > "$evidence/result.txt"' EXIT
git rev-parse HEAD > "$evidence/revision.txt"
git status --porcelain=v1 > "$evidence/working-tree.txt"

for stage in check bindings-check e2e-perf; do
    printf '%s\n' "$stage" > "$evidence/current-stage.txt"
    printf '\nStarting %s\n' "$stage"
    if make "$stage" 2>&1 | tee "$evidence/$stage.log"; then
        printf '%s\t0\n' "$stage" >> "$evidence/stages.tsv"
    else
        status=$?
        printf '%s\t%s\n' "$stage" "$status" >> "$evidence/stages.tsv"
        printf 'Failed %s; evidence: %s\n' "$stage" "$evidence" >&2
        exit "$status"
    fi
done
printf 'All pre-push gates passed; evidence: %s\n' "$evidence"

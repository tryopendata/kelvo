#!/bin/bash
# Stop hook: format + lint only files changed in this session.
# Uses git to find staged + unstaged changes, then routes each file
# to the right formatter. Skips untracked files (new files not yet added).
#
# All commands redirect stderr to /dev/null to avoid spurious "hook error"
# messages from Claude Code, which can cause a stop-hook feedback loop.

# Script-relative fallback: a Stop hook fired from a cwd outside any work tree
# (scratchpad, /tmp) used to exit here and leave the session's edits unformatted.
repo_root="$(git rev-parse --show-toplevel 2>/dev/null || (cd "$(dirname "$0")/../.." && pwd))"
cd "$repo_root" || exit 0

# Collect unique changed files (staged + unstaged, excluding deleted)
changed_files=$(git diff --name-only --diff-filter=d HEAD 2>/dev/null; git diff --name-only --diff-filter=d --cached 2>/dev/null)
changed_files=$(echo "$changed_files" | sort -u | grep -v '^$' || true)

[[ -z "$changed_files" ]] && exit 0

# Split by toolchain
frontend_files=()
rust_changed=0

while IFS= read -r file; do
    [[ ! -f "$repo_root/$file" ]] && continue
    case "$file" in
        src/core/generated/*) ;;
        src/*.ts|src/*.tsx|src/*.css|tests/*.ts|tests/*.tsx)
                        frontend_files+=("$repo_root/$file") ;;
        *.rs)           rust_changed=1 ;;
    esac
done <<< "$changed_files"

# Format + lint-fix frontend
if [[ ${#frontend_files[@]} -gt 0 ]]; then
    cd "$repo_root"
    bunx biome check --write --no-errors-on-unmatched "${frontend_files[@]}" >/dev/null 2>&1 || true
    # No tsc here: a full project typecheck on every Stop is too slow, and CI +
    # pre-commit already enforce it. Keep Stop to file-scoped formatting.
fi

# Format Rust. The workspace root Cargo.toml arrives with the crates; until
# then src-tauri is the only package.
if [[ $rust_changed -eq 1 ]]; then
    for dir in "$repo_root" "$repo_root/src-tauri"; do
        if [[ -f "$dir/Cargo.toml" ]]; then
            (cd "$dir" && cargo fmt --all) 2>/dev/null || true
            break
        fi
    done
fi

exit 0

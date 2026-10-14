#!/bin/bash
# Stop hook: format + lint only files changed in this session.
# lint-on-write.sh records session writes; unrelated dirty files are not ours.
# All formatter output is suppressed to avoid Stop-hook feedback loops.
input=$(cat)
session_id=$(printf '%s' "$input" | jq -r '.session_id // empty')
repo_root="$(cd "$(dirname "$0")/../.." && pwd)"
source "$(dirname "$0")/_session-files.sh"
session_file=$(session_files_path "$repo_root" "$session_id") || exit 0
[[ -f "$session_file" ]] || exit 0
cd "$repo_root" || exit 0

# Split by toolchain
frontend_files=()
rust_changed=0

while IFS= read -r -d '' file; do
    [[ ! -f "$repo_root/$file" ]] && continue
    case "$file" in
        src/core/generated/*) ;;
        src/*.ts|src/*.tsx|src/*.css|tests/*.ts|tests/*.tsx)
                        # A file may be written many times in one turn.
                        already_listed=false
                        for listed in "${frontend_files[@]}"; do
                            [[ "$listed" == "$repo_root/$file" ]] && already_listed=true
                        done
                        [[ "$already_listed" == true ]] || frontend_files+=("$repo_root/$file") ;;
        *.rs)           rust_changed=1 ;;
    esac
done < "$session_file"

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

#!/bin/bash
# Per-checkout, per-session write journal. Keep it in the worktree's private
# Git directory so simultaneous sessions/checkouts never share formatting scope.
# NUL delimiters preserve spaces/newlines in file names. No session ID means no
# automatic Stop autofix; whole-workspace formatting remains explicitly usable.
session_files_path() {
    local repo_root="$1" session_id="$2" state_dir key
    [[ -n "$session_id" ]] || return 1
    state_dir=$(git -C "$repo_root" rev-parse --absolute-git-dir 2>/dev/null) || return 1
    state_dir="$state_dir/kelvo-hook-sessions"
    key=$(printf '%s' "$session_id" | shasum -a 256 | awk '{print $1}')
    [[ -n "$key" ]] || return 1
    mkdir -p "$state_dir" || return 1
    printf '%s/%s.files' "$state_dir" "$key"
}

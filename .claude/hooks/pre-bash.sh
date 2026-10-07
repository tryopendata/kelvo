#!/bin/bash
# PreToolUse/Bash dispatcher: one process, one stdin read, several checks.
#
# Ported from opendata's pre-bash.sh. Its rr/uv rewriters and typecheck steering
# are dropped (Kelvo runs everything locally); the destructive-command deny,
# sleep-poll deny, generated-file write guard, worktree anchor and flag fixes
# are kept.
#
# MERGE SEMANTICS:
#   1. Deny beats everything. Deny checks run before any rewrite is emitted, so
#      a compound command like "git commit --no-verify && rm -rf /" can never be
#      laundered by a rewriter.
#   2. First rewrite wins (only block-destructive fixes rewrite today).
#   3. worktree-anchor is context-only and combines with whatever decision is
#      emitted.
#
# The command lives at .tool_input.command in the real Claude Code payload.
# (.command is accepted as a fallback so the hook is testable with either shape.)

input=$(cat)
command=$(echo "$input" | jq -r '.tool_input.command // .command // empty')

# The agent's cwd, which is NOT this process's cwd in general. Claude Code sends
# it as a top-level `cwd` field on every hook payload; that is the contract, and
# $PWD only happens to agree because of how the hook is currently spawned. Fall
# back to $PWD so the hook stays usable when driven by hand or from tests.
hook_cwd=$(echo "$input" | jq -r '.cwd // empty')
hook_cwd=${hook_cwd:-$PWD}

# Resolved once: several checks need it, and each one otherwise costs a subshell.
# Resolve it against the agent's cwd, not this process's, so the answer is right
# inside a worktree (--show-toplevel returns the worktree root).
repo_root=$(cd "$hook_cwd" 2>/dev/null && git rev-parse --show-toplevel 2>/dev/null)

# Result accumulators, filled by the check functions below.
DENY_REASON=""        # non-empty -> emit deny
REWRITE_REASON=""     # non-empty (with REWRITE_CMD) -> emit allow+updatedInput
REWRITE_CMD=""
ANCHOR_CONTEXT=""     # non-empty -> merge additionalContext into the output

# shellcheck source=_destructive-patterns.sh
source "$(dirname "$0")/_destructive-patterns.sh"

# --- Check 1a: destructive command deny (must run before any rewriter) -------
check_block_destructive_deny() {
    local dz
    if dz=$(is_destructive "$command"); then
        local reason="${dz%%$'\t'*}"
        local suggestion="${dz#*$'\t'}"
        DENY_REASON="BLOCKED: $reason"$'\n\n'"$suggestion"
    fi
}

# --- Check 1b: sleep-poll deny ------------------------------------------------
# Deny bare sleep-to-wait / sleep-then-poll patterns. Agents use bare `sleep N`
# to wait for a process, blocking the session unproductively.
# Passes through: sleep inside until/while/for loops, complex multi-statement
# scripts (3+ separators), fractional/variable sleep args.
check_sleep_poll_deny() {
    [[ -n "$DENY_REASON" ]] && return 0

    # Passthrough: sleep already inside a proper loop (idiomatic bounded wait).
    echo "$command" | grep -qE '\b(until|while|for)\b' && return 0

    # Passthrough: complex multi-statement scripts (3+ &&/; separators).
    local sep_count
    sep_count=$(echo "$command" | grep -oE '(&&|;)' | wc -l | tr -d ' ')
    [[ "$sep_count" -ge 3 ]] && return 0

    # Narrow deny patterns (integer sleep args only):
    #   sleep 30                        (bare sleep)
    #   sleep 60 && curl .../health     (sleep-then-check)
    #   curl .../health; sleep 30       (do-then-sleep, manual poll)
    local is_sleep_poll=false
    echo "$command" | grep -qE '^sleep\s+[0-9]+\s*$'    && is_sleep_poll=true
    echo "$command" | grep -qE '^sleep\s+[0-9]+\s*&&'   && is_sleep_poll=true
    echo "$command" | grep -qE ';\s*sleep\s+[0-9]+\s*$' && is_sleep_poll=true
    [[ "$is_sleep_poll" == "false" ]] && return 0

    DENY_REASON="Don't block on bare sleep. Instead use one of:

  1. Condition-based wait (preferred):
       wait-for 1420    (or: until curl -sf http://localhost:1420; do sleep 2; done)
     Run this as a single Bash call -- it polls every 10 s until the condition succeeds.

  2. Background task + Monitor:
       Run the long job with run_in_background=true, then use the Monitor tool
       to stream its output. No sleep needed.

Either approach avoids blocking the session and gives real signal when the work is done."
}

# --- Check 1c: block Bash writes to generated bindings -----------------------
# The Edit|Write|MultiEdit path is guarded by block-generated-edits.sh, but that
# hook only matches those three tools. A shell write -- `cp x.ts
# src/core/generated/bindings.ts`, a redirect, `tee`, `sed -i`, `mv` -- would
# sail straight through. The tauri-specta export (make bindings) is the
# sanctioned path and passes.
check_block_generated_writes() {
    [[ -n "$DENY_REASON" ]] && return 0

    # Passthrough: the sanctioned regenerator.
    echo "$command" | grep -qE '(make[[:space:]]+bindings|export-bindings)' && return 0

    local gen='(^|[[:space:]/])src/core/generated'

    local is_write=false
    echo "$command" | grep -qE ">>?[[:space:]]*[^[:space:]]*src/core/generated" && is_write=true
    echo "$command" | grep -qE '(^|[[:space:]|;&])(cp|mv|tee|install|rsync)[[:space:]]' \
        && echo "$command" | grep -qE "$gen" && is_write=true
    echo "$command" | grep -qE '(^|[[:space:]|;&])sed[[:space:]].*-i' \
        && echo "$command" | grep -qE "$gen" && is_write=true

    [[ "$is_write" == "false" ]] && return 0

    DENY_REASON="BLOCKED: don't write src/core/generated/ from a shell command.

These TypeScript bindings are generated by tauri-specta from the Rust types,
commands and events. Hand-written or copied bindings drift from Rust silently.

Do this instead:
  - Change the Rust type/command, run \`make bindings\`, and commit the
    regenerated files with the change (see .claude/rules/generated-bindings.md)."
}

# --- Check 2: worktree anchor (context-only, order-free) ----------------------
# Surface worktree context once per session on the first Bash call. Uses
# CLAUDE_CODE_SESSION_ID + repo root as a stable flag key.
check_worktree_anchor() {
    # The global repo_root is resolved from the agent's cwd. When that cwd is
    # outside any work tree, fall back to the checkout this script lives in
    # instead of returning early and never showing the anchor.
    local repo_root="${repo_root:-$(cd "$(dirname "$0")/../.." && pwd)}"

    local session_key flag
    session_key="$(echo "$repo_root" | tr '/' '-')-${CLAUDE_CODE_SESSION_ID:-unknown}"
    flag="/tmp/.wt-anchored-${session_key}"

    [[ -f "$flag" ]] && return 0
    touch "$flag"

    local worktrees cwd
    worktrees="$(git -C "$repo_root" worktree list 2>/dev/null || echo 'not a git repo')"
    cwd="$hook_cwd"

    ANCHOR_CONTEXT="[Session start] Working directory context:
CWD: ${cwd}
Worktrees:
${worktrees}

Confirm you are working in the correct worktree before reading or editing files."
}

# --- Check 3: block-destructive silent fixes ----------------------------------
# Strip risky-but-fixable flags rather than denying.
check_block_destructive_fixes() {
    local fixed="$command"
    local reason=""

    # Strip --no-verify from git commits
    if echo "$fixed" | grep -qE 'git\s+commit.*--no-verify'; then
        fixed=$(echo "$fixed" | sed -E 's/\s*--no-verify//g')
        reason="Stripped --no-verify"
    fi

    # Strip --no-gpg-sign
    if echo "$fixed" | grep -qE 'git\s+commit.*--no-gpg-sign'; then
        fixed=$(echo "$fixed" | sed -E 's/\s*--no-gpg-sign//g')
        reason="${reason:+$reason + }Stripped --no-gpg-sign"
    fi

    # Downgrade git branch -D to -d
    if echo "$fixed" | grep -qE 'git\s+branch\s+-D\s'; then
        fixed=$(echo "$fixed" | sed 's/-D/-d/')
        reason="${reason:+$reason + }Downgraded -D to -d"
    fi

    if [[ "$fixed" != "$command" ]]; then
        # Clean up whitespace
        fixed=$(echo "$fixed" | sed 's/  */ /g' | sed 's/ *$//')
        REWRITE_REASON="$reason"
        REWRITE_CMD="$fixed"
    fi
}

# === RUN CHECKS ================================================================

# Anchor is command-independent; run it even when the payload has no command
# so first-Bash-call context still fires (matches the old standalone hook,
# which never inspected the command).
check_worktree_anchor

if [[ -n "$command" ]]; then
    # Deny checks FIRST: a compound command must never reach a rewriter before
    # the deny screen, and deny outranks any rewrite (Claude Code semantics).
    check_block_destructive_deny
    check_sleep_poll_deny
    check_block_generated_writes

    # Rewriters only when nothing denied.
    if [[ -z "$DENY_REASON" ]]; then
        check_block_destructive_fixes
    fi
fi

# === EMIT ONE MERGED JSON ======================================================

# Every context-only check appends here; additionalContext is a single string,
# so contributors have to concatenate rather than assign, or the last one to
# run silently drops the others.
CONTEXT="$ANCHOR_CONTEXT"

if [[ -n "$DENY_REASON" ]]; then
    jq -n --arg reason "$DENY_REASON" --arg ctx "$CONTEXT" '{
      hookSpecificOutput: ({
        hookEventName: "PreToolUse",
        permissionDecision: "deny",
        permissionDecisionReason: $reason
      } + (if $ctx != "" then {additionalContext: $ctx} else {} end))
    }'
    exit 0
fi

if [[ -n "$REWRITE_CMD" ]]; then
    jq -n --arg reason "$REWRITE_REASON" --arg cmd "$REWRITE_CMD" --arg ctx "$CONTEXT" '{
      hookSpecificOutput: ({
        hookEventName: "PreToolUse",
        permissionDecision: "allow",
        permissionDecisionReason: $reason,
        updatedInput: { command: $cmd }
      } + (if $ctx != "" then {additionalContext: $ctx} else {} end))
    }'
    exit 0
fi

if [[ -n "$CONTEXT" ]]; then
    jq -n --arg ctx "$CONTEXT" '{
      hookSpecificOutput: {
        hookEventName: "PreToolUse",
        additionalContext: $ctx
      }
    }'
    exit 0
fi

exit 0

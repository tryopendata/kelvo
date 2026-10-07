#!/bin/bash
# SessionStart hook: inject a couple of lines of live repo state that CLAUDE.md
# can't carry because it's static - current branch, how dirty the tree is, and
# where to look first. Keeps the agent oriented from turn one.
#
# Guarded: if we're not in a git repo, emit nothing and exit 0.
# Cheap: a few git calls, no network. Output kept well under ~300 tokens.
#
# NOTE: SessionStart fires for the main session only, not subagents. Subagent
# CWD/worktree orientation is handled by the anchor check in pre-bash.sh (a PreToolUse hook,
# which does fire inside subagents). These two are complementary, not dupes.

cat >/dev/null  # drain stdin (payload unused)
repo_root=$(git rev-parse --show-toplevel 2>/dev/null) || exit 0

branch=$(git -C "$repo_root" rev-parse --abbrev-ref HEAD 2>/dev/null)
dirty=$(git -C "$repo_root" status --porcelain 2>/dev/null | wc -l | tr -d ' ')

# Are we in a linked worktree (not the main checkout)? If so, say which.
wt_note=""
common_dir=$(git -C "$repo_root" rev-parse --git-common-dir 2>/dev/null)
git_dir=$(git -C "$repo_root" rev-parse --git-dir 2>/dev/null)
if [[ -n "$common_dir" && "$common_dir" != "$git_dir" ]]; then
    wt_note=" (in a linked worktree at $repo_root)"
fi

lines="Repo state: branch \`$branch\`$wt_note, $dirty changed file(s)."
lines="$lines Before UI work, read plan/design-system.md and the screens already built, and check plan/PROGRESS.md for status."
if [[ "$branch" == "main" ]]; then
    lines="$lines NOTE: you're on \`main\` - branch before committing non-trivial work."
fi

jq -n --arg ctx "$lines" '{
  hookSpecificOutput: {
    hookEventName: "SessionStart",
    additionalContext: $ctx
  }
}'
exit 0

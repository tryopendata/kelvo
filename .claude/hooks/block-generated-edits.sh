#!/bin/bash
# PreToolUse hook: block manual edits/writes to generated files.
#
# src/core/generated/ holds the TypeScript bindings tauri-specta generates from
# the Rust commands, events and kelvo-schema types. Rust is the source of
# truth; a hand edit drifts silently until the next regen wipes it. Making the
# files un-editable converts that churn into "can't happen": change the Rust
# side and regenerate.
#
# Blocks with the explicit permissionDecision: deny JSON form.

input=$(cat)
tool_name=$(echo "$input" | jq -r '.tool_name // empty')
case "$tool_name" in
    Edit|Write|MultiEdit) ;;
    *) exit 0 ;;
esac

file_path=$(echo "$input" | jq -r '.tool_input.file_path // empty')
[[ -z "$file_path" ]] && exit 0

# Match the generated artifacts anywhere in the path (worktrees included).
if [[ "$file_path" == */src/core/generated/* \
   || "$file_path" == src/core/generated/* ]]; then
    reason="BLOCKED: $(basename "$file_path") is auto-generated.

src/core/generated/ is produced by tauri-specta from the Rust types, commands
and events. Don't edit it by hand, and don't hand-write a TS mirror of a Rust
type elsewhere either.

Change the Rust side (derive specta::Type / register the command or event),
run \`make bindings\`, and commit the regenerated files with the change.
See .claude/rules/generated-bindings.md."
    jq -n --arg r "$reason" '{
      hookSpecificOutput: {
        hookEventName: "PreToolUse",
        permissionDecision: "deny",
        permissionDecisionReason: $r
      }
    }'
    exit 0
fi

exit 0

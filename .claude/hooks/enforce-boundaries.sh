#!/bin/bash
# PreToolUse hook: enforce architecture boundaries on file writes.
# Denies violations that can't be silently fixed (need design decisions).
#
# Boundaries (biome.json's noRestrictedImports overrides enforce the same set;
# this catches them at write time):
# 1. src/core/ must NEVER import React or from ~/ (framework-agnostic layer)
# 2. src/app/widgets/ is render-only: no transport, stores, router, or Tauri APIs

input=$(cat)

deny() {
    jq -n --arg reason "$1" '{
      hookSpecificOutput: {
        hookEventName: "PreToolUse",
        permissionDecision: "deny",
        permissionDecisionReason: $reason
      }
    }'
    exit 0
}

tool_name=$(echo "$input" | jq -r '.tool_name // empty')
file_path=$(echo "$input" | jq -r '.tool_input.file_path // empty')

[[ -z "$file_path" ]] && exit 0

# Extract the text being written
if [[ "$tool_name" == "Edit" ]]; then
    text=$(echo "$input" | jq -r '.tool_input.new_string // empty')
elif [[ "$tool_name" == "Write" ]]; then
    text=$(echo "$input" | jq -r '.tool_input.content // empty')
elif [[ "$tool_name" == "MultiEdit" ]]; then
    text=$(echo "$input" | jq -r '[.tool_input.edits[]?.new_string // empty] | join("\n")')
else
    exit 0
fi

[[ -z "$text" ]] && exit 0

# --- Rule 1: src/core/ must not import React or the app layer ---
if [[ "$file_path" =~ /src/core/.*\.(ts|tsx)$ && "$file_path" != */src/core/generated/* ]]; then
    if printf '%s' "$text" | grep -qE -e "from[[:space:]]+[\"'](~/|react|react-dom|zustand/react|@tanstack/react-query)([\"'/])"; then
        deny "BLOCKED: Architecture boundary violation.

src/core/ is the framework-agnostic layer (zero React dependencies).
It must not import React, React bindings, or anything from ~/ (src/app/).

Options:
  - Keep the pure logic in @core/ and put the hook/component in src/app/
  - Pass React-specific values into core functions as parameters"
    fi
fi

# --- Rule 2: src/app/widgets/ is render-only ---
if [[ "$file_path" =~ /src/app/widgets/.*\.(ts|tsx)$ ]]; then
    if printf '%s' "$text" | grep -qE -e "from[[:space:]]+[\"'](@core/transport|@tauri-apps/|react-router|zustand|@tanstack/react-query|~/stores/|~/routes/)"; then
        deny "BLOCKED: Architecture boundary violation.

src/app/widgets/ components are render-only. They take data and callbacks as
props so the same component renders in the popover, overview cards, board
windows and composer preview, and its prop contract matches the v3 WidgetKit
feed. They must not import @core/transport, stores, react-router, TanStack
Query, or @tauri-apps/*.

Options:
  - Subscribe/fetch in the route or a hook under src/app/hooks/, pass values down
  - Take a callback prop (onOpen, onSelect) instead of navigating or invoking"
    fi
fi

exit 0

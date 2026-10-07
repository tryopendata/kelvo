#!/bin/bash
# PreToolUse hook: auto-approve safe tools to reduce permission prompts.
# Runs AFTER pre-bash.sh (which catches dangerous commands first).
#
# Exit 0 with permissionDecision JSON = auto-approve without prompting.
# Exit 0 with no output = defer to normal permission flow (may prompt user).

INPUT=$(cat)
tool=$(echo "$INPUT" | jq -r '.tool_name // empty')

approve() {
    jq -n --arg reason "$1" '{
      hookSpecificOutput: {
        hookEventName: "PreToolUse",
        permissionDecision: "allow",
        permissionDecisionReason: $reason
      }
    }'
    exit 0
}

# --- Always safe: read-only tools ---
case "$tool" in
    Read|Glob|Grep|Skill|WebFetch|WebSearch|TaskCreate|TaskUpdate|TaskGet|TaskList|TodoRead)
        approve "Read-only tool" ;;
    mcp__ide__getDiagnostics|mcp__plugin_context7_context7__resolve-library-id|mcp__plugin_context7_context7__query-docs)
        approve "Read-only MCP tool" ;;
esac

# --- Always safe: file editing (detect-secrets hook screens these separately) ---
case "$tool" in
    Edit|Write|MultiEdit|NotebookEdit)
        approve "File edit (screened by detect-secrets hook)" ;;
esac

# --- Bash: approve known-safe command prefixes ---
if [[ "$tool" == "Bash" ]]; then
    command=$(echo "$INPUT" | jq -r '.tool_input.command // .command // empty')

    # Never auto-approve a command pre-bash.sh would reject. Both hooks
    # consult the same pattern list, so auto-approve can't rubber-stamp something
    # the blocker denies, regardless of the order Claude Code runs the two
    # PreToolUse groups in. (Defense in depth; "deny" already beats "allow".)
    # shellcheck source=_destructive-patterns.sh
    source "$(dirname "$0")/_destructive-patterns.sh"
    if is_destructive "$command" >/dev/null; then
        exit 0  # defer to normal flow + pre-bash.sh's deny; do NOT approve
    fi

    # Auto-approve classifies on the FIRST WORD only. If the command chains or
    # redirects (a && rm, cat > /dev/sda, find . -exec rm {} +, cmd | sink),
    # the innocent first word would rubber-stamp the whole thing. is_destructive
    # catches the known-bad shapes; as defense in depth we also refuse to
    # auto-approve ANY command carrying an unanalyzed metachar or -exec and let
    # the normal permission flow decide.
    # A lone & counts too: `timeout 5 rm x & other` is two commands, and the
    # rm check in is_destructive is scoped to one command at a time.
    if echo "$command" | grep -qE '(;|&|\||>|<|`|\$\()' || echo "$command" | grep -qE '\s-exec(dir)?(\s|$)'; then
        exit 0
    fi

    # Strip leading env vars (FOO=bar cmd → cmd)
    base_cmd=$(echo "$command" | sed -E 's/^([A-Z_]+=[^ ]+ +)*//')

    # Extract first word (the actual command)
    first_word=$(echo "$base_cmd" | awk '{print $1}')

    # An executing wrapper runs an rm this file approves on the wrapper's name
    # alone, and is_destructive scans an rm inside quotes only up to the next
    # quote (so `grep "rm -rf" .` isn't a false positive). Close that gap here,
    # narrowly: defer (normal prompt, not a deny) only when the wrapped rm has
    # a dangerous target: a bare / ~ . .. or *, or one starting with $ or ~/.
    # `bash -c 'rm -rf build dist'` and `timeout 30 rm -f /tmp/x.json` stay
    # approved.
    case "$first_word" in
        bash|sh|zsh|eval|xargs|timeout|time|find|env|sudo|nohup|command|exec|ssh)
            if echo "$command" | grep -qE "(^|[^[:alnum:]_.-])rm[[:space:]][^;&|]*([[:space:]]|=)['\"]?((/|~/?|\.\.?|\*)([[:space:]'\")]|$)|\\$|~/)"; then
                exit 0
            fi ;;
    esac

    # --- Subcommand-aware approval (demoted from blanket first-word) ---
    if [[ "$first_word" == "docker" ]]; then
        sub=$(echo "$base_cmd" | awk '{print $2}')
        case "$sub" in
            ps|logs|inspect|images|info|version|stats|top|port|diff|context)
                approve "Docker read-only subcommand" ;;
            exec)
                echo "$base_cmd" | grep -qE '^docker\s+exec\s+(-\S+\s+)*\S+\s+(cat|ls|head|tail|grep|env|ps|wc|find|stat)\b' \
                    && approve "Docker exec read-only" ;;
        esac
        exit 0
    fi

    if [[ "$first_word" == "gh" ]]; then
        two=$(echo "$base_cmd" | awk '{print $2" "$3}')
        case "$two" in
            "pr view"|"pr list"|"pr diff"|"pr checks"|"pr create"|"pr comment"|\
            "run list"|"run view"|"run rerun"|\
            "issue view"|"issue list"|"repo view"|"auth status")
                approve "gh allowlisted subcommand" ;;
        esac
        if echo "$base_cmd" | grep -qE '^gh\s+api\b' \
           && ! echo "$base_cmd" | grep -qE -- '(-X|--method)[= ]*(POST|PUT|PATCH|DELETE)|(^|\s)-(f|F)(\s|=)|--input|--field|--raw-field'; then
            approve "gh api GET"
        fi
        exit 0
    fi

    if [[ "$first_word" == "curl" ]]; then
        # All methods against localhost are safe (dev environment only)
        if echo "$base_cmd" | grep -qE '(localhost|127\.0\.0\.1|0\.0\.0\.0)'; then
            approve "curl against localhost"
        fi
        if ! echo "$base_cmd" | grep -qE -- '(-X|--request)[= ]*(POST|PUT|PATCH|DELETE)|(^|\s)-d(\s|=)|--data|(^|\s)-F(\s|=)|--form|--upload-file|(^|\s)-T(\s|=)'; then
            approve "curl GET"
        fi
        exit 0
    fi

    # Safe read-only commands
    case "$first_word" in
        ls|pwd|tree|cat|head|tail|wc|which|env|test|echo|printf|date|hostname|whoami|id|uname|file|stat|du|df)
            approve "Read-only shell command" ;;
    esac

    # Dev tools
    case "$first_word" in
        bun|bunx|npm|npx|node|make|cargo|rustup|rustc|wait-for|on-port|free-port)
            approve "Dev tool" ;;
    esac

    # Safe git operations (destructive ones already blocked by pre-bash.sh)
    case "$first_word" in
        git)
            approve "Git command (destructive ops blocked by separate hook)" ;;
    esac

    # Infrastructure / debugging
    case "$first_word" in
        jq|grep|find|xargs|sort|uniq|diff|comm|tee|timeout|time|mkdir|chmod|cp|mv|touch|ln|basename|dirname|realpath|readlink)
            approve "Shell utility" ;;
    esac

    # Piped/chained commands: check if it starts with a safe command
    # The pre-bash.sh destructive screen already checked for dangerous patterns,
    # so if we got here the chain doesn't contain blocked commands.
    case "$first_word" in
        set|cd|export|if|for|while|bash)
            approve "Shell construct (destructive ops blocked by separate hook)" ;;
    esac
fi

# --- Playwright MCP tools ---
case "$tool" in
    mcp__plugin_playwright_playwright__*)
        approve "Playwright browser tool" ;;
esac

# --- Everything else: defer to normal permission flow ---
exit 0

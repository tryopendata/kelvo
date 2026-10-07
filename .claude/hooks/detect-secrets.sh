#!/bin/bash
# PreToolUse hook: scan file writes for hardcoded secrets.
# Checks new_string (Edit) or content (Write) for high-confidence secret patterns.
# Exit 0 = allow or deny (via JSON permissionDecision).
#
# Design: only flags values that LOOK like real secrets (high entropy, known formats).
# Ignores placeholders, env var references, and safe file types.

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

# Only check Edit and Write tools
[[ "$tool_name" != "Edit" && "$tool_name" != "Write" && "$tool_name" != "MultiEdit" ]] && exit 0

# Skip safe file types where secrets in context are expected/acceptable
case "$file_path" in
    *.example|*.md|*.txt|*.rst|*.html|*.lock|*.svg|*.png|*.jpg|*.ico)
        exit 0 ;;
    */.env.example|*/CLAUDE.md|*/.gitignore)
        exit 0 ;;
esac

# Extract the text being written
if [[ "$tool_name" == "Edit" ]]; then
    text=$(echo "$input" | jq -r '.tool_input.new_string // empty')
elif [[ "$tool_name" == "Write" ]]; then
    text=$(echo "$input" | jq -r '.tool_input.content // empty')
elif [[ "$tool_name" == "MultiEdit" ]]; then
    text=$(echo "$input" | jq -r '[.tool_input.edits[]?.new_string // empty] | join("\n")')
fi

[[ -z "$text" ]] && exit 0

# Helper: grep that won't choke on content starting with dashes.
# Uses printf to avoid echo interpretation issues, and -e to mark the pattern.
check() {
    printf '%s' "$text" | grep -qE -e "$1"
}

# --- Pattern matching ---
findings=""

# AWS access keys (AKIA + 16 alphanumeric)
if check 'AKIA[0-9A-Z]{16}'; then
    findings="${findings}- AWS Access Key (AKIA...)\n"
fi

# AWS secret keys (40-char base64 after common assignment patterns)
if check '(aws_secret_access_key|AWS_SECRET)\s*=\s*[A-Za-z0-9/+=]{40}'; then
    findings="${findings}- AWS Secret Key\n"
fi

# GitHub tokens (ghp_, gho_, ghs_, ghu_, github_pat_)
if check '(ghp_[A-Za-z0-9]{36}|gho_[A-Za-z0-9]{36}|ghs_[A-Za-z0-9]{36}|ghu_[A-Za-z0-9]{36}|github_pat_[A-Za-z0-9_]{22,})'; then
    findings="${findings}- GitHub Token\n"
fi

# Slack tokens/webhooks
if check '(xox[baprs]-[A-Za-z0-9-]{10,}|hooks\.slack\.com/services/T[A-Z0-9]+/B[A-Z0-9]+/[A-Za-z0-9]+)'; then
    findings="${findings}- Slack Token/Webhook\n"
fi

# Stripe live keys (sk_live_, rk_live_ with substantial key material)
if check '(sk_live_[A-Za-z0-9]{20,}|rk_live_[A-Za-z0-9]{20,})'; then
    findings="${findings}- Stripe Live Key\n"
fi

# OpenAI keys (sk-... with the T3BlbkFJ middle segment)
if check 'sk-[A-Za-z0-9]{20,}'; then
    # Only flag if it looks like a real OpenAI key (48+ chars)
    if printf '%s' "$text" | grep -qE -e 'sk-[A-Za-z0-9]{48,}'; then
        findings="${findings}- Possible OpenAI API Key\n"
    fi
fi

# Anthropic keys (sk-ant-api03-...). Needs its own pattern: the hyphens in the
# prefix break the OpenAI rule's [A-Za-z0-9] class at "sk-ant", so it never reaches
# the 48-char length gate above.
if check 'sk-ant-[A-Za-z0-9_-]{20,}'; then
    # The [A-Za-z0-9_-] class swallows underscore/hyphen-separated placeholder
    # words ("REPLACE_ME", "your-api-key"), so guard the same way the generic
    # secret rule below does.
    if ! printf '%s' "$text" | grep -qiE -e '(your-|changeme|replace|example|placeholder|xxx|fake|dummy|TODO|FIXME)'; then
        findings="${findings}- Anthropic API Key\n"
    fi
fi

# Private keys (PEM format)
if check 'BEGIN (RSA |EC |DSA |OPENSSH )?PRIVATE KEY'; then
    findings="${findings}- Private Key (PEM format)\n"
fi

# Connection strings with embedded passwords (user:password@host pattern)
if check '(postgresql|mysql|mongodb|redis|amqp)://[^:]+:[^@]{8,}@'; then
    findings="${findings}- Database connection string with embedded password\n"
fi

# JWT tokens (three dot-separated base64url segments)
if check 'eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.[A-Za-z0-9_-]{10,}'; then
    findings="${findings}- JWT token\n"
fi

# Clerk secret keys
if check 'sk_test_[A-Za-z0-9]{20,}|sk_live_[A-Za-z0-9]{20,}'; then
    # Don't double-report if already caught as Stripe
    if ! printf '%s' "$findings" | grep -q "Stripe"; then
        findings="${findings}- Clerk/Stripe Secret Key\n"
    fi
fi

# PostHog API keys (phc_ prefix)
if check 'phc_[A-Za-z0-9]{20,}'; then
    findings="${findings}- PostHog API Key\n"
fi

# Tauri updater signing key (minisign secret key, base64 "untrusted comment" blob)
if check 'untrusted comment: (rsign|minisign) encrypted secret key'; then
    findings="${findings}- Tauri updater / minisign secret key\n"
fi

# Apple notarization app-specific password (xxxx-xxxx-xxxx-xxxx)
if check '(APPLE_PASSWORD|APPLE_APP_SPECIFIC_PASSWORD)\s*[:=]\s*["\x27]?[a-z]{4}-[a-z]{4}-[a-z]{4}-[a-z]{4}'; then
    findings="${findings}- Apple app-specific password\n"
fi

# Sentry DSN (contains secret in URL)
if check 'https://[a-f0-9]{32}@[a-z0-9.]+\.ingest\.sentry\.io'; then
    findings="${findings}- Sentry DSN\n"
fi

# Generic: long high-entropy values assigned to secret-looking variable names
# Only match actual values, not placeholder/example strings
if check '(api[_-]?key|api[_-]?secret|secret[_-]?key|access[_-]?token|auth[_-]?token|private[_-]?key)\s*[:=]\s*["\x27][A-Za-z0-9/+=_-]{32,}["\x27]'; then
    # Exclude known placeholders
    if ! printf '%s' "$text" | grep -qiE -e '(your-|changeme|replace|example|placeholder|xxx|fake|dummy|TODO|FIXME)'; then
        findings="${findings}- Possible hardcoded secret (long key/token value)\n"
    fi
fi

# --- Report findings ---
if [[ -n "$findings" ]]; then
    deny "BLOCKED: Possible secret detected in write to $(basename "$file_path")

Detected:
$(echo -e "$findings")
If these are intentional (test fixtures, examples), ask the user to confirm.
For real credentials, use environment variables or a secrets manager instead."
fi

exit 0

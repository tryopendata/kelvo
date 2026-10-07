#!/bin/bash
# Shared destructive-command detection, sourced by both pre-bash.sh
# (which emits a deny decision) and auto-approve.sh (which refuses to approve).
#
# Single source of truth: if a pattern is added here, auto-approve can never
# rubber-stamp a command that block-destructive would reject, regardless of
# the order Claude Code runs the two PreToolUse hook groups in.
#
# Usage:
#   source "$(dirname "$0")/_destructive-patterns.sh"
#   if reason=$(is_destructive "$command"); then ... fi
# Returns 0 (with a human-readable reason + suggestion on stdout, tab-separated)
# when the command is destructive, 1 otherwise.

# _is_artifact_find <segment>: true when the segment is a find that deletes only
# build/cache artifacts, and nothing else:
#   find <relative in-repo path> [-type d|f] [-maxdepth N] [-mindepth N]
#        -name|-iname <artifact> ... (-delete | -exec rm [-f|-r|-rf] {} + | \;)
# <artifact> is one of the names below (quoted or not). Anything outside that
# grammar (-o, -or, `,`, !, -not, -newer, -path, an absolute/~/../$var path, a
# source pattern like '*.py' or '*') is not an artifact find, so the find rule
# still applies to it.
_is_artifact_find() {
    local -a t
    read -ra t <<< "$1"
    [[ "${t[0]:-}" == find ]] || return 1
    local path="${t[1]:-}"
    [[ "$path" =~ ^[A-Za-z0-9_.][A-Za-z0-9_./-]*$ && "$path" != *..* ]] || return 1
    local i=2 n=${#t[@]} named=false pat
    while (( i < n )); do
        case "${t[i]}" in
            -name|-iname)
                pat="${t[i+1]:-}"; pat="${pat#[\'\"]}"; pat="${pat%[\'\"]}"
                case "$pat" in
                    __pycache__|'*.pyc'|'*.pyo'|.DS_Store|.pytest_cache|.ruff_cache|.mypy_cache|node_modules|'*.log'|'*.tmp') named=true ;;
                    *) return 1 ;;
                esac
                i=$((i + 2)) ;;
            -type)
                [[ "${t[i+1]:-}" == [fd] ]] || return 1; i=$((i + 2)) ;;
            -maxdepth|-mindepth)
                [[ "${t[i+1]:-}" =~ ^[0-9]+$ ]] || return 1; i=$((i + 2)) ;;
            -delete)
                (( i == n - 1 )) || return 1; break ;;
            -exec)
                [[ "${t[i+1]:-}" == rm ]] || return 1
                local j=$((i + 2))
                [[ "${t[j]:-}" =~ ^-[rf]{1,2}$ ]] && j=$((j + 1))
                [[ "${t[j]:-}" == '{}' && ( "${t[j+1]:-}" == '+' || "${t[j+1]:-}" == '\;' ) ]] || return 1
                (( j + 2 == n )) || return 1
                break ;;
            *) return 1 ;;
        esac
    done
    (( i < n )) || return 1   # no -delete / -exec action reached
    [[ "$named" == true ]]
}

# _strip_artifact_find <command>: print the command with every separator-delimited
# segment that is an artifact-only find removed. Other segments are kept
# verbatim, so this can only exempt that grammar, never widen it. `\;` (find's
# -exec terminator) is protected from the split.
_strip_artifact_find() {
    local seg trimmed
    while IFS= read -r seg; do
        trimmed="${seg#"${seg%%[![:space:]]*}"}"
        trimmed="${trimmed%"${trimmed##*[![:space:]]}"}"
        _is_artifact_find "${trimmed//__ESC_SEMI__/\\;}" && continue
        printf '%s\n' "${seg//__ESC_SEMI__/\\;}"
    done < <(printf '%s\n' "${command//\\;/__ESC_SEMI__}" | sed -E 's/(&&|\|\||[;&|])/\
/g')
}

# _blank_inert_quotes: stdin -> stdout, with the body of every quoted string
# that no executing consumer runs replaced by underscores (quotes kept). See
# the rm rule in is_destructive. A line whose pipeline feeds ssh or a shell
# (echo "rm -rf ~" | ssh host) runs its quoted text, so it is left as is.
_blank_inert_quotes() {
    awk -v consumer='(^|[[:space:](])((ba|z|da)?sh[[:space:]]+-[a-z]*c[a-z]*|eval|su[[:space:]]+-c|rr[[:space:]]+run([[:space:]].*)?|ssh([[:space:]].*)?|xargs([[:space:]].*)?|docker[[:space:]]+exec([[:space:]].*)?)[[:space:]]+$' '
    /\|[[:space:]]*(ssh|(ba|z|da)?sh)([[:space:]]|$)/ { print; next }
    {
        line = $0; out = ""; seg = ""; n = length(line); i = 1
        while (i <= n) {
            c = substr(line, i, 1)
            if (c == "\\") { t = substr(line, i, 2); out = out t; seg = seg t; i += 2; continue }
            if (c == "\047" || c == "\"") {
                k = i + 1
                while (k <= n && substr(line, k, 1) != c) {
                    if (c == "\"" && substr(line, k, 1) == "\\") k++
                    k++
                }
                if (k > n) { out = out substr(line, i); break }
                body = substr(line, i + 1, k - i - 1)
                keep = (seg ~ consumer) || (c == "\"" && (index(body, "$(") || index(body, "`")))
                if (!keep) gsub(/./, "_", body)
                t = c body c; out = out t; seg = seg t; i = k + 1; continue
            }
            if (c == ";" || c == "&" || c == "|") seg = ""; else seg = seg c
            out = out c; i++
        }
        print out
    }'
}

is_destructive() {
    local command="$1"
    [[ -z "$command" ]] && return 1

    # Each block: if the pattern matches, print "<reason>\t<suggestion>" and return 0.

    # --- Filesystem destruction ---
    # rm with a dangerous target (/, ~, ~/, ., ..), independent of how the
    # recursive/force flags are arranged. We deliberately do NOT require -r/-f:
    # `rm /` or `rm ~` against these targets is dangerous enough to stop and ask,
    # and matching on the target alone closes the split-flag bypass
    # (`rm -r -f /`, `rm -f -r ~`) that a flag-cluster regex misses.
    #
    # Locating rm: any `rm` word preceded by start, whitespace, a separator, a
    # paren or a backtick, optionally as a path (/bin/rm) or escaped (\rm). So
    # wrapper words in front (sudo env nohup time timeout command exec) never
    # hide it, nor does x;rm, (rm ...), $(rm ...). `git rm` and `docker volume
    # rm` are matched too, as they were before.
    #
    # The target must sit in the SAME command as the rm. The scan from rm to
    # the target is shell-aware in the ways that matter: it stops at an
    # unquoted ; & or |, but steps over whole '...' and "..." strings, \x
    # escapes, $( ... ) and `...` substitutions, so a separator inside any of
    # those (rm -rf "x;" /, rm -rf a\;b /, rm -rf $(ls|head -1) ~) does not end
    # it, while `rm -f x && echo "done" && df -h /` stops at the &&. Redirects
    # (2>&1, >&2, &>) are blanked first so their & is not read as a separator.
    # A dangerous target is one of / ~ ~/ . .. as its own argument (preceded by
    # whitespace or =, followed by whitespace, a separator, a closing
    # quote/paren/backtick, a redirect, or end of line).
    #
    # A quoted string is only code when an executing consumer runs it: sh/bash/
    # zsh/dash -c, eval, su -c, rr run, ssh, xargs, docker exec. Every other
    # quoted string (rg 'rm -rf ~', git log --grep="rm -rf /", echo "...") is
    # blanked before the scan, so mentioning a dangerous rm is not running one.
    # A double-quoted string holding $( or a backtick is kept, since the shell
    # runs that substitution.
    #
    # An rm that opens a consumer's quoted string (bash -c 'rm -rf /') is
    # scanned only up to the next quote. Wrapped rm with inner quotes
    # (bash -c 'rm -rf "x" ~') is left to auto-approve, which refuses to
    # approve it; over ssh, where the host allowlist would run it without a
    # prompt, the rule below denies it.
    #
    # $( ) is stepped over only without nested parens; an rm whose command
    # holds $( ... ( falls back to a greedy scan to the end of the line.
    local sq="'" dq='"' bt='`'
    local rm_cmd='\\?([^[:space:];&|]*/)?rm[[:space:]]'
    local rm_unit="([^;&|${sq}${dq}\\\\${bt}]|${sq}[^${sq}]*${sq}|${dq}[^${dq}]*${dq}|\\\\.|\\\$\\([^()]*\\)|${bt}[^${bt}]*${bt})"
    local rm_bare="(^|[[:space:];&|(${bt}])${rm_cmd}${rm_unit}*([[:space:]]|=)"
    local rm_quoted="[${sq}${dq}]${rm_cmd}[^;&|${sq}${dq}]*([[:space:]]|=)"
    local rm_end="([[:space:];&|)<>${bt}${sq}${dq}]|\\\\n|\$)"
    local rm_danger="(${rm_bare}|${rm_quoted})"
    local rm_text
    rm_text=$(printf '%s\n' "$command" | sed -E 's/[0-9]*>&[0-9-]*/ /g; s/&>/ /g' | _blank_inert_quotes)
    if printf '%s\n' "$rm_text" | grep -qE "(^|[[:space:];&|(${bt}${sq}${dq}])${rm_cmd}[^;&|]*\\$\\([^)]*\\("; then
        rm_danger="(^|[[:space:];&|(${bt}${sq}${dq}])${rm_cmd}.*([[:space:]]|=)"
    fi
    if printf '%s\n' "$rm_text" | grep -qE "${rm_danger}(/|~/?|\.\.?)${rm_end}"; then
        printf '%s\t%s' "Destructive rm targeting root, home, or working directory." "Specify the exact path you want to remove. Ask the user first."
        return 0
    fi
    if printf '%s\n' "$rm_text" | grep -qE "${rm_danger}\*${rm_end}"; then
        printf '%s\t%s' "rm with a bare wildcard." "Be explicit about which files to remove. Ask the user first."
        return 0
    fi
    # ssh runs its quoted argument as a remote command, and a host allowlist
    # would let it run with no prompt. Inside
    # an ssh command, any rm whose own command reaches a dangerous target is
    # denied, quotes included (ssh linux-local 'rm -rf "/tmp/x" ~'). ssh counts
    # anywhere as a command word, so timeout/nohup/time/env X=1 ssh match too.
    if printf '%s\n' "$rm_text" | grep -qE "(^|[[:space:];&|(])ssh[[:space:]]" \
       && printf '%s\n' "$rm_text" | grep -qE "(^|[[:space:];&|(${bt}${sq}${dq}])${rm_cmd}[^;&|]*([[:space:]]|=)(/|~/?|\.\.?|\*)${rm_end}"; then
        printf '%s\t%s' "rm targeting root, home, or working directory inside an ssh remote command." "This would run on the remote box. Name the exact paths. Ask the user first."
        return 0
    fi
    if echo "$command" | grep -qE 'xargs\s+[^;&|]*(rm\s+-rf|kill\s+-9|chmod\s+777)'; then
        printf '%s\t%s' "Destructive command piped through xargs." "Review the pipeline carefully. Ask the user first."
        return 0
    fi
    # find ... -delete / find ... -exec rm  is the most common way an agent
    # bulk-deletes a tree. auto-approve allowlists `find`, so without this it
    # would be actively approved, not just unblocked.
    #
    # Exempt: a segment that is an artifact-only cleanup (see
    # _is_artifact_find), e.g. `find scripts -name '*.pyc' -delete` or
    # `find . -name __pycache__ -type d -exec rm -rf {} +`. Such segments are
    # dropped before the check; everything else is checked as before.
    local find_re='\bfind\b.*-(delete\b|exec\s+(rm|unlink|shred|trash)\b)'
    if echo "$command" | grep -qE "$find_re" \
       && _strip_artifact_find "$command" | grep -qE "$find_re"; then
        printf '%s\t%s' "find with -delete or -exec rm deletes every matched file." "Review the find expression and target path. Ask the user first."
        return 0
    fi
    # Writing a raw block device is unrecoverable; cat/echo/dd are all allowlisted.
    if echo "$command" | grep -qE '>\s*/dev/(sd|nvme|hd|disk|mapper|vd|xvd)'; then
        printf '%s\t%s' "Redirecting output to a raw disk device destroys it." "This overwrites a block device. Do not run without explicit confirmation."
        return 0
    fi
    if echo "$command" | grep -qE '\bdd\b.*\bof=/dev/(sd|nvme|hd|disk|mapper|vd|xvd)'; then
        printf '%s\t%s' "dd writing to a raw disk device destroys it." "Confirm the of= target is not a real disk. Ask the user first."
        return 0
    fi
    # mkfs / format against a device wipes the filesystem.
    if echo "$command" | grep -qE '\bmkfs(\.\w+)?\s+/dev/'; then
        printf '%s\t%s' "mkfs reformats a device, destroying all data on it." "Confirm the device. Do not run without explicit confirmation."
        return 0
    fi

    # --- Git: blowing away uncommitted work ---
    if echo "$command" | grep -qE 'git\s+checkout\s+(--\s+)?\.(\s|;|&&|\||$)'; then
        printf '%s\t%s' "git checkout . discards ALL unstaged changes." "If you need to revert specific files, use: git checkout -- <file>"
        return 0
    fi
    if echo "$command" | grep -qE 'git\s+restore\s+\.(\s|;|&&|\||$)'; then
        printf '%s\t%s' "git restore . discards ALL unstaged changes." "If you need to revert specific files, use: git restore <file>"
        return 0
    fi
    if echo "$command" | grep -qE 'git\s+clean\s+-[a-zA-Z]*f'; then
        printf '%s\t%s' "git clean -f deletes untracked files permanently." "Ask the user before removing untracked files."
        return 0
    fi
    if echo "$command" | grep -qE 'git\s+reset\s+--hard'; then
        printf '%s\t%s' "git reset --hard discards ALL uncommitted changes (staged and unstaged)." "Use git reset (soft) or git stash if you need to preserve work."
        return 0
    fi
    if echo "$command" | grep -qE 'git\s+stash\s+(drop|clear)'; then
        printf '%s\t%s' "git stash drop/clear permanently destroys stashed work." "Other agents may have work in the stash. Ask the user first."
        return 0
    fi
    if echo "$command" | grep -qE 'git\s+stash\s*&&'; then
        printf '%s\t%s' "git stash chained with another command can destroy parallel agent work." "Other agents may be working on files. Ask the user before stashing."
        return 0
    fi
    if echo "$command" | grep -qE 'git\s+stash\s+pop\s*&&\s*git\s+stash\s+drop'; then
        printf '%s\t%s' "git stash pop && drop is a destructive pattern." "Ask the user first - other agents may have stashed work."
        return 0
    fi

    # --- Git: force push (allow --force-with-lease as the safe alternative) ---
    if echo "$command" | grep -qE 'git\s+push\s+(-[a-zA-Z]*f|--force)(\s|$)'; then
        printf '%s\t%s' "Force push rewrites remote history." "Ask the user before force pushing. Use --force-with-lease if you must."
        return 0
    fi

    # --- Database destruction ---
    if echo "$command" | grep -qiE '(drop\s+(table|database|schema)|truncate\s+table|delete\s+from\s+\S+\s*;?\s*$)'; then
        printf '%s\t%s' "Destructive database operation detected." "DROP/TRUNCATE/unfiltered DELETE can cause irreversible data loss. Ask the user first."
        return 0
    fi

    # --- Process killing (broad) ---
    if echo "$command" | grep -qE 'kill\s+-9\s+-1|killall|pkill\s+-9'; then
        printf '%s\t%s' "Aggressive process killing." "Use targeted kill with a specific PID, or ask the user."
        return 0
    fi

    # --- chmod 777 ---
    if echo "$command" | grep -qE 'chmod\s+(-R\s+)?777'; then
        printf '%s\t%s' "chmod 777 makes files world-writable." "Use more restrictive permissions (755 for dirs, 644 for files)."
        return 0
    fi

    # --- Unrecoverable platform destruction ---
    if echo "$command" | grep -qE '\bdocker\s+(system\s+prune|volume\s+rm|rmi)\b'; then
        printf '%s\t%s' "docker system prune / volume rm / rmi destroys images or volumes." "Ask the user first."
        return 0
    fi
    if echo "$command" | grep -qE '\bgh\s+(repo|release)\s+delete\b'; then
        printf '%s\t%s' "gh repo/release delete is unrecoverable." "Ask the user first."
        return 0
    fi
    if echo "$command" | grep -qE '\b(npm|bun|uv|cargo)\s+publish\b' && ! echo "$command" | grep -qE -- '--dry-run'; then
        printf '%s\t%s' "Publishing a package is irreversible." "Releases are user-initiated. Ask the user first."
        return 0
    fi
    if echo "$command" | grep -qE '\bpsql\b' && echo "$command" | grep -qiE '\b(drop|truncate)\b'; then
        printf '%s\t%s' "psql carrying DROP or TRUNCATE." "Irreversible schema/data destruction. Ask the user first."
        return 0
    fi

    return 1
}

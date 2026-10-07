#!/bin/bash
# Stop / SubagentStop hook: kill orphaned dev-tool processes left behind by
# this repo's checkouts.
#
# When an agent's Bash call is cut off (timeout, interrupt, a killed parent),
# tsc / vitest workers / eslint / mypy can be reparented to PID 1 and keep a
# core busy for minutes. With several agents in parallel worktrees that adds up
# to a saturated laptop. This sweeps them up when a turn ends.
#
# Deliberately narrow. A tool process is reaped only if ALL of these hold:
#   - the program is tsc, vitest (incl. its fork/tinypool workers), eslint,
#     mypy (not dmypy), playwright, or `vite build` -- as argv[0], or as the
#     script an interpreter (node/bun/python) is running. A tool name appearing
#     later in argv (`rr run 'bunx vitest ...'`) does not count,
#   - it is orphaned: either its own parent is PID 1, or every process between
#     it and PID 1 is a launcher (bun, bunx, uv, uvx, npx, `sh -c`, `bash -c`,
#     or node running a checkout script) -- what `bun run typecheck` and
#     `uv run mypy` leave behind. Then that topmost launcher's whole tree goes,
#   - the topmost process has run longer than REAP_ORPHANS_MIN_AGE seconds (120),
#   - the tool's or a launcher's command line names a path inside one of this
#     repo's checkouts (from `git worktree list`; sibling repos don't count),
#   - it is ours (ps -U <uid>) and not an ancestor of this hook.
#
# cargo, rustc and pytest are not in the tool list, so they never match.
# nohup execs its command, so a nohup'd tool run looks like any other orphan:
# to keep one alive on purpose, give it a non-launcher parent.
# Bare `vite` / `vite dev` / `vite preview` are servers and are left alone.
#
# Fast (one ps, one git call), never fails the session (always exits 0), and
# logs one line to stderr when it kills something.
#
# Test seams: REAP_ORPHANS_PS_FILE replaces the ps listing (lines of
# "pid ppid etime command..."); REAP_ORPHANS_ROOTS replaces the checkout roots
# (newline-separated); REAP_ORPHANS_SELF stands in for this hook's pid;
# REAP_ORPHANS_DRY_RUN=1 sends no signals.

cat >/dev/null 2>&1   # hook payload; nothing in it is needed

min_age=${REAP_ORPHANS_MIN_AGE:-120}

hook_root=$(cd "$(dirname "$0")/../.." 2>/dev/null && pwd)
roots=${REAP_ORPHANS_ROOTS-$(git -C "$hook_root" worktree list --porcelain 2>/dev/null | sed -n 's/^worktree //p')}
roots=$(printf '%s' "$roots" | tr '\n' '|')
[[ -z "$roots" ]] && exit 0

if [[ -n "${REAP_ORPHANS_PS_FILE:-}" ]]; then
    listing=$(cat "$REAP_ORPHANS_PS_FILE" 2>/dev/null)
else
    listing=$(ps -ww -U "$(id -u)" -o pid=,ppid=,etime=,command= 2>/dev/null)
fi

# Output: one line per victim tree, "<top pid> <tool> <pid> <pid>..." (the
# top's whole subtree, top first).
victims=$(printf '%s\n' "$listing" | awk -v min_age="$min_age" -v roots="$roots" -v self="${REAP_ORPHANS_SELF:-$$}" '
    function secs(t,   d, n, p, s, i) {          # etime: [[dd-]hh:]mm:ss
        d = 0
        if (index(t, "-")) { d = substr(t, 1, index(t, "-") - 1); t = substr(t, index(t, "-") + 1) }
        n = split(t, p, ":"); s = 0
        for (i = 1; i <= n; i++) s = s * 60 + p[i]
        return d * 86400 + s
    }
    function base(p) { sub(/.*\//, "", p); return p }
    function names_checkout(p,   i) {
        for (i = 1; i <= nr; i++) if (r[i] != "" && index(cm[p], r[i] "/")) return 1
        return 0
    }
    function tool_of(p,   b0, b1) {
        b0 = base(a0[p]); b1 = base(a1[p])
        if (b0 ~ /^(tsc|vitest|eslint|mypy|playwright)$/) return b0
        if (b0 == "vite") return (a1[p] == "build") ? "vite" : ""
        if (b0 ~ /^(node|bun|python[0-9.]*)$/) {
            if (b1 ~ /^(tsc|vitest|eslint|mypy|playwright)$/) return b1
            if (b1 == "vite") return (a2[p] == "build") ? "vite" : ""
            if (a1[p] ~ /\/node_modules\/(vitest|tinypool)\//) return "vitest"
            if (a1[p] ~ /\/node_modules\/(@playwright\/test|playwright)\//) return "playwright"
            if (a1[p] == "-m" && a2[p] == "mypy") return "mypy"
        }
        return ""
    }
    function launcher(p,   b0) {
        b0 = base(a0[p])
        if (b0 ~ /^(bun|bunx|uv|uvx|npx)$/) return 1
        if (b0 ~ /^(sh|bash)$/) return a1[p] == "-c"
        if (b0 == "node") return names_checkout(p)
        return 0
    }
    function subtree(p,   out, k) {
        out = " " p
        for (k = 1; k <= n; k++) if (pp[order[k]] == p) out = out subtree(order[k])
        return out
    }
    BEGIN { nr = split(roots, r, "|") }
    NF >= 4 {
        p = $1; pp[p] = $2; et[p] = secs($3); a0[p] = $4; a1[p] = $5; a2[p] = $6
        c = $0; sub(/^[ \t]*[0-9]+[ \t]+[0-9]+[ \t]+[^ \t]+[ \t]+/, "", c); cm[p] = c
        order[++n] = p
    }
    END {
        for (q = self; q in pp; q = pp[q]) mine[q] = 1     # never our own ancestry
        for (k = 1; k <= n; k++) {
            p = order[k]; t = tool_of(p)
            if (t == "") continue
            top = p; named = names_checkout(p); ok = 1; depth = 0
            while (pp[top] != 1) {
                up = pp[top]
                if (!(up in pp) || ++depth > 4 || !launcher(up)) { ok = 0; break }
                top = up
                if (names_checkout(top)) named = 1
            }
            if (!ok || !named || et[top] <= min_age || (top in mine) || (top in done)) continue
            done[top] = 1
            print top, t subtree(top)
        }
    }')

[[ -z "$victims" ]] && exit 0

killed=""
while read -r top tool pids; do
    # shellcheck disable=SC2086  # $pids is a list
    [[ -z "${REAP_ORPHANS_DRY_RUN:-}" ]] && kill -TERM $pids 2>/dev/null
    killed+="${killed:+, }$top $tool"
done <<< "$victims"

[[ -n "$killed" ]] && echo "reap-orphans: killed orphaned dev processes: $killed" >&2
exit 0

#!/bin/bash
# PostToolUse hook: auto-format and lint files after Edit/Write/MultiEdit.
# Files are formatted on every write (biome format, rustfmt, trailing
# whitespace + EOF) so they're always clean on disk and pre-commit's pure
# auto-fix hooks don't cost a commit cycle. Lint/type errors are surfaced via
# additionalContext so the agent sees them and can fix inline.
#
# Formatters only, never lint autofix: `biome check --write`
# would delete an import the agent added one Edit before the code that uses
# it. Those run at Stop (format-changed.sh) and in pre-commit.

input=$(cat)

tool_name=$(echo "$input" | jq -r '.tool_name // empty')

# Only care about file-writing tools
case "$tool_name" in
    Edit|Write|MultiEdit) ;;
    *) exit 0 ;;
esac

# Extract file path from tool input
file_path=$(echo "$input" | jq -r '.tool_input.file_path // empty')

[[ -z "$file_path" ]] && exit 0
[[ ! -f "$file_path" ]] && exit 0

# Resolve the repo from the edited file, not the cwd. The cwd can be outside any
# work tree (scratchpad, /tmp), or in the main checkout while the file being
# edited lives in a worktree; either way the file's own checkout is the one
# whose toolchain and config apply. --show-prefix gives the path relative to that
# root without string-matching absolute paths (which /tmp -> /private/tmp and
# other symlinks break).
file_dir="$(dirname "$file_path")"
repo_root="$(git -C "$file_dir" rev-parse --show-toplevel 2>/dev/null)" || exit 0
rel_path="$(git -C "$file_dir" rev-parse --show-prefix 2>/dev/null)$(basename "$file_path")"

# Only lint files inside a Kelvo checkout (another repo can share the src/
# layout; its files are not ours to format).
[[ -f "$repo_root/.claude/hooks/lint-on-write.sh" ]] || exit 0

# Trailing whitespace + end-of-file, byte-for-byte what pre-commit-hooks'
# trailing-whitespace and end-of-file-fixer do (types: [text]). grep -I treats a file
# with NUL bytes as binary. Gitignored files are skipped: pre-commit never sees
# them. The file is rewritten only when something changes, so a clean file
# keeps its mtime and the agent's view of it stays current.
if grep -Iq . "$file_path" 2>/dev/null && ! git -C "$repo_root" check-ignore -q -- "$rel_path" 2>/dev/null; then
    fix_eof=1
    perl -e '
        my ($f, $fix_eof) = @ARGV;
        open(my $in, "<:raw", $f) or exit 0;
        my $s = do { local $/; <$in> };
        close $in;
        my $orig = $s;
        my @lines = split /(?<=\n)/, $s, -1;
        for (@lines) {
            my $eol = s/(\r?\n)\z// ? $1 : "";
            s/[ \t\x0b\x0c\r]+\z//;
            $_ .= $eol;
        }
        $s = join "", @lines;
        if ($fix_eof) {
            if ($s =~ /[^\r\n]/) { $s =~ s/(\r\n|\n|\r)[\r\n]*\z/$1/ or $s .= "\n"; }
            else { $s = ""; }
        }
        if ($s ne $orig) {
            open(my $out, ">:raw", $f) or exit 0;
            print $out $s;
            close $out;
        }
    ' "$file_path" "$fix_eof" 2>/dev/null || true
fi

# Biome. Format on write (formatter only: no lint fixes, no import
# organizing, which would drop an import added one Edit before its use), then
# lint report-only. This is where the core/widget boundary rules surface.
# Run from the repo root so biome.json resolves; biome.json's files.includes
# skips src/core/generated/. The local binary is fast; bunx is the fallback.
biome_bin="$repo_root/node_modules/.bin/biome"
[[ -x "$biome_bin" ]] || biome_bin="bunx biome"

errors=""
notes=""

case "$rel_path" in
    src/core/generated/*)
        ;;
    *.ts|*.tsx|*.js|*.mjs|*.json|*.jsonc|*.css)
        (cd "$repo_root" && $biome_bin format --write --no-errors-on-unmatched "$rel_path") >/dev/null 2>&1 || true
        if [[ "$rel_path" == *.ts || "$rel_path" == *.tsx ]]; then
            lint_out=$(cd "$repo_root" && $biome_bin lint --no-errors-on-unmatched --max-diagnostics=20 "$rel_path" 2>&1) || true
            if echo "$lint_out" | grep -q "×"; then
                errors+="$lint_out"$'\n'
            fi
        fi
        ;;
    *.rs)
        # rustfmt on the single file, using the workspace rustfmt.toml. clippy
        # is a whole-crate build, too slow for a per-write hook; it runs at
        # pre-commit and in CI.
        if command -v rustfmt >/dev/null 2>&1; then
            rustfmt --edition 2024 --config-path "$repo_root/rustfmt.toml" "$file_path" >/dev/null 2>&1 || true
        fi
        ;;
esac

# No issues found
if [[ -z "$errors" && -z "$notes" ]]; then
    exit 0
fi

# Surface errors (and skip notes) so the agent sees them and can fix inline
msg=""
[[ -n "$errors" ]] && msg="Lint/type errors in $rel_path:"$'\n'"$errors"
[[ -n "$notes" ]] && msg+="$rel_path: $notes"
jq -n --arg errors "$msg" \
    '{ "hookSpecificOutput": { "hookEventName": "PostToolUse", "additionalContext": $errors } }'

exit 0

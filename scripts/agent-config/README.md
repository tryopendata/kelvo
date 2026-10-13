# Shared agent guidance

Claude Code's `CLAUDE.md` and `.claude/` are the source of truth. This adapter
exposes that guidance to other harnesses without changing the Claude setup.
It follows OpenData's agent-config approach introduced in commit `48e2ba7ed`.

| Output | Consumer | Source |
| --- | --- | --- |
| `AGENTS.md` | Codex and other AGENTS.md readers | `CLAUDE.md`, rule paths, command descriptions, compatibility notes in `sync.py` |
| `.agents/skills/<name>` | Codex and Antigravity | Relative directory link to `.claude/skills/<name>` |
| `.agents/rules/<name>.md` | Antigravity | `.claude/rules/`, with converted activation frontmatter |

All paths in this document are relative to the repository root. Outputs are
committed, so a fresh clone or worktree has the guidance without a setup step.
Codex uses the rule table in `AGENTS.md` to read applicable source rules; it does
not automatically load Antigravity's `.agents/rules/` files.

## Maintenance

1. Edit the original Claude instruction, rule, skill, or command.
2. Add new shared rules and skills to `scripts/agent-config/shared.txt`.
   Commands under `.claude/commands/*.md` are indexed automatically.
3. Run `make agent-config`, then `make agent-config-check agent-config-test`.
4. Commit the source changes and generated outputs together. Never hand-edit
   `AGENTS.md` or generated Antigravity rules.

`sync.py` uses Python 3.9+ and the standard library. It resolves the checkout from
its own location, so `python3 /path/to/checkout/scripts/agent-config/sync.py --check`
also works from a nested directory or outside the checkout. No home-directory
configuration is installed. Run Make targets from the checkout root.

`--check` writes nothing: exit 0 means current, 1 means drift, and 2 means an
invalid source or output collision. Pre-commit checks relevant changes; Linux CI
checks drift and runs the synchronizer tests. Unsupported frontmatter fails
explicitly instead of silently changing which paths activate a rule.

For a native Codex discovery smoke check, run `codex debug prompt-input` from the
root and from `src/app/`. Its prompt preview should contain the generated project
instructions and a `frontend-design-slop` entry in the available-skills catalog.
The CLI may display skill paths using an alias from its skill-roots table. This
preview does not start a model turn; it can also include personal instructions,
so do not commit or publish the full output.

The synchronizer removes only files with its generated marker and skill symlinks
into `.claude/skills/`. It refuses to overwrite hand-written output files or use
symlinked output directories. Keep hand-written additions separate from its
reserved output names. Relative skill links continue to work when the checkout
moves and in Git worktrees; filesystem tools must distinguish directory symlinks
from regular files when scanning the repository.

## Personal plugin skills

Repository skills already use relative directory links. Personal Claude Essentials
skills use the same pattern at home: `~/.agents/skills/<name>` points to
`~/.claude/plugins/marketplaces/claude-essentials/plugins/ce/skills/<name>`.
The whole directory is linked so its `references/` resolve without copied files.
`architecting-systems` and `planning-products` are the two planning skills used
by Kelvo; their Claude plugin aliases are `ce:architecting-systems` and
`ce:planning-products`.

This follows OpenData's `scripts/agent-config/setup-home.sh` pattern, but that
script only enumerates personal `~/.claude/skills/*`, not plugin skills. Kelvo's
synchronizer deliberately does not change home configuration. Create each personal
link only when its source `SKILL.md` exists and its destination is absent; leave
collisions untouched. Prefer the marketplace path over a versioned cache path.
Do not copy or run OpenData's broader global-instruction/import-sync migration.

After linking, a fresh `codex debug prompt-input` should list both skills with
paths ending in `<name>/SKILL.md`. Inspect only the skill entries; the rest of the
prompt can contain private configuration. If a requested `ce:` skill is missing,
follow the lookup instructions in canonical `CLAUDE.md` before declaring it absent.

## Harness behavior

Claude keeps its existing instructions, skills, slash commands, permissions, and
hooks. Other harnesses read the shared instructions and follow the command links
using their own shell tools; `$ARGUMENTS` in a Claude command means the caller's
supplied arguments. Missing external plugins are not installed by this adapter.

This shares guidance, not runtime hook enforcement. Use existing Make/Bun/Cargo
checks and `make hooks` for the harness-independent Git hooks. No delegation
runner, model configuration, access restrictions, or hook adapters are added.

Codex supports repository `AGENTS.md` and symlinked `.agents/skills/` folders:
[instruction discovery](https://learn.chatgpt.com/docs/agent-configuration/agents-md),
[skill discovery](https://learn.chatgpt.com/docs/build-skills).

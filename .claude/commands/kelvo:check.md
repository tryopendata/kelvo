---
description: Run full quality checks (format, lint, typecheck, test) for frontend and Rust
disable-model-invocation: true
---

Run the complete quality gate and fix any issues found.

## Step 1: Pre-commit hooks (all files)

Runs every configured hook: Biome, TypeScript, rustfmt, clippy, plus the
whitespace, YAML/TOML, secret and large-file checks.

```bash
uvx pre-commit run --all-files
```

> Use `uvx pre-commit` (not bare `pre-commit`) unless pre-commit is on PATH. `uvx` fetches
> the pinned version and runs it against `.pre-commit-config.yaml`.

If hooks auto-fix files, re-run once to confirm clean.

## Step 2: Everything CI runs

```bash
make check
```

That is `biome check` (format + lint), `tsc`, Vitest, `cargo fmt --check`, clippy with
`-D warnings` and `cargo test`. It is the final gate; during iteration use scoped runs
(see `.claude/rules/testing.md`).

## Success Criteria

- Format: no changes needed (Biome, rustfmt)
- Lint: Biome clean, clippy clean with `-D warnings`
- Typecheck: `tsc` clean
- Tests: Vitest and cargo test passing, with a non-zero test count

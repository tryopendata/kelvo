---
description: Run tests (scoped by default)
argument-hint: [path-or-filter]
disable-model-invocation: true
---

Scoped runs are the default during iteration; escalate only as far as needed (the full
ladder is in `.claude/rules/testing.md`).

```bash
bun run test -- src/core/format          # Frontend, scoped by path substring
bun run test -- -t "bytes"               # Frontend, filter by test name
cargo test -p kelvo-store rollup        # Rust, one crate + name filter
bun run test                             # Whole frontend suite, when the change is done
make test                                # Frontend + Rust, final gate before commit
```

With `$ARGUMENTS`, run the scoped form that matches it.

.PHONY: help install dev dev-web build test test-e2e test-fill test-read-perf lint format fix typecheck check rust-fmt rust-check rust-test rust-linux-check appstore-check check-deps deny perf bench bench-perf-mode bench-vs-stats bindings bindings-check e2e-perf hooks clean stub-dist

LINUX_CRATES := -p kelvo-schema -p kelvo-proto -p kelvo-store -p kelvo-engine -p kelvo-collect

help:
	@echo "Kelvo Development Commands"
	@echo ""
	@echo "Setup:"
	@echo "  make install       Install JS deps and git hooks"
	@echo ""
	@echo "Development:"
	@echo "  make dev           Full app (tauri dev: Vite on :1420 + Rust)"
	@echo "  make dev-web       Browser only, mock transport"
	@echo "  make build         Production app bundle"
	@echo "  make bindings      Regenerate src/core/generated/ from Rust (tauri-specta)"
	@echo ""
	@echo "Quality:"
	@echo "  make test          Frontend + Rust tests"
	@echo "  make test-e2e      Playwright against the dev server + mock transport"
	@echo "  make test-fill     30-day synthetic store fill, asserts the 150 MB budget (release, ~1 min)"
	@echo "  make test-read-perf  30-day Timeline, heatmap and CSV export timings against perf-budget.json (release)"
	@echo "  make lint          Biome lint + clippy -D warnings"
	@echo "  make format        Biome (format + safe fixes) + cargo fmt"
	@echo "  make fix           Auto-fix formatting and lint issues"
	@echo "  make typecheck     tsc + cargo check"
	@echo "  make check         Format check, lint, typecheck, tests, frontend + Rust (the pre-push hook runs this)"
	@echo "  make bindings-check  Regenerate the bindings and fail if they differ from the commit"
	@echo "  make e2e-perf      Playwright frontend perf gates (perf-budget.json frontend; macOS budgets)"
	@echo "  make rust-check    cargo fmt --check + clippy -D warnings"
	@echo "  make rust-linux-check  cargo check the portable crates for x86_64-unknown-linux-gnu"
	@echo "  make appstore-check    cargo check + clippy the sandboxed App Store edition (--features appstore)"
	@echo "  make check-deps    Fail if the built app links a library macOS does not ship (D-058)"
	@echo "  make deny          cargo-deny: advisories, licenses, bans, sources (deny.toml); skips if not installed"
	@echo "  make perf          Release engine CPU, 120 s tray-only at 1 s (then a 30 s run); fails over perf-budget.json's baseline (D-062, D-067, D-075)"
	@echo "  make bench         Whole app + WebKit helpers: tray, popover, Overview, Processes; distance to the 0.5% target (D-067)"
	@echo "  make bench-perf-mode  Performance mode off vs on in pairs: tray, tray on battery, Overview (D-088)"
	@echo "  make bench-vs-stats  Tray-only Kelvo vs Stats.app for 10 minutes each (skips if Stats is not installed)"

install:
	bun install
	uvx pre-commit install --install-hooks -t pre-commit -t commit-msg -t pre-push

hooks:
	uvx pre-commit install --install-hooks -t pre-commit -t commit-msg -t pre-push

dev:
	bun run tauri dev

dev-web:
	bun run dev

build:
	bun run tauri build

test: rust-test
	bun run test

test-e2e:
	bun run test:e2e

# Ignored in `cargo test` because it writes ~1 GB through SQLite. CI runs it on Linux.
test-fill:
	cargo test --release -p kelvo-store --test fill -- --ignored --nocapture

# The 30-day read budget alone (perf-budget.json `store`): Timeline 30d, heatmap, CSV export.
test-read-perf:
	cargo test --release -p kelvo-store --test fill thirty_day_reads -- --ignored --nocapture

lint: stub-dist
	bun run lint
	cargo clippy --workspace --all-targets -- -D warnings

format: rust-fmt
	bun run format

fix: format
	bun run lint:fix

typecheck: stub-dist
	bun run typecheck
	cargo check --workspace --all-targets

check: rust-check appstore-check rust-test
	bun run check

rust-fmt:
	cargo fmt --all

rust-check: stub-dist
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings

rust-test: stub-dist
	cargo test --workspace

# The sandboxed App Store edition (v3.2) must keep compiling: the feature drops the
# private-API collectors at registration, compiles out the responsible-PID lookup and
# turns process signals off (D-065).
appstore-check: stub-dist
	cargo check -p kelvo --features appstore --all-targets
	cargo clippy -p kelvo -p kelvo-engine -p kelvo-collect --features kelvo/appstore --all-targets -- -D warnings

rust-linux-check:
	rustup target add x86_64-unknown-linux-gnu
	cargo check --target x86_64-unknown-linux-gnu $(LINUX_CRATES)

# Kelvo installs nothing and requires nothing (D-058): every linked library must come
# with macOS. Checks the debug binary; build it first (bun tauri build --debug).
check-deps:
	scripts/check-deps.sh

# Dependency audit (deny.toml). CI runs it in the `deny` job; locally it needs
# cargo-deny on PATH and skips with a message without it.
deny:
	@if command -v cargo-deny >/dev/null 2>&1; then \
		cargo deny --all-features check; \
	else \
		echo "deny: cargo-deny is not installed, skipping (CI runs it; to run it here: cargo install --locked cargo-deny)"; \
	fi

# Engine CPU gate (D-062, D-067, D-075): the release engine for 120 s tray-only at 1 s,
# then a 30 s interval run for comparison, one at a time. Baseline in perf-budget.json
# (engine.perf).
perf:
	scripts/perf.sh

# Whole-app CPU and footprint (D-067): builds the bench bundle and measures each
# scenario against perf-budget.json (coalition). Keep hands off while it runs.
bench:
	scripts/bench-coalition.sh

# Performance mode saving (D-088): off/on pairs (parallel bundles tray-only, alternating
# on the dashboard) against perf-budget.json coalition.performanceMode. Hands off, on AC.
bench-perf-mode:
	scripts/bench-perf-mode.sh

bench-vs-stats:
	scripts/bench-vs-stats.sh

# tauri::generate_context! embeds frontendDist (../dist) at compile time, so any
# cargo build of src-tauri needs the directory to exist, even empty.
stub-dist:
	@mkdir -p dist

# Writes src/core/generated/bindings.ts from the tauri-specta builder in a debug
# build of the app shell, without launching the app. The pre-push hook diffs the result.
bindings: stub-dist
	cargo run --quiet -p kelvo --example export_bindings

# Main-thread and long-task budgets (perf-budget.json `frontend`), measured on macOS
# Chromium, so they run in the pre-push hook rather than on Linux CI (D-096).
e2e-perf:
	bunx playwright test tests/e2e/perf-gate.spec.ts --project=chromium

# Fails if src/core/generated differs from the commit after regenerating, including
# new untracked files.
bindings-check: bindings
	@if [ -n "$$(git status --porcelain -- src/core/generated)" ]; then \
		git status --porcelain -- src/core/generated; \
		echo "bindings-check: src/core/generated is stale. Run 'make bindings' and commit the result."; \
		exit 1; \
	fi

clean:
	rm -rf dist coverage playwright-report test-results
	cargo clean

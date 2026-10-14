# Kelvo architecture refactor plan

Review date: 2026-10-14. Baseline: `7b0b900` on local `main` (nine commits ahead of `origin/main`). Status: implementation complete and reviewed on `codex/architecture-refactor`; execution evidence follows the owner-revised plan below.

Kelvo demonstrates real system-design ability. Its strongest evidence is the series model, explicit missing-data semantics, single SQLite writer, generated Rust/TypeScript contracts, scoped React stores, deterministic engine tests and measured performance constraints. The production TypeScript import graph inspected in this review contains no cycles or core-to-app edges. This foundation should remain.

The main weakness is uneven enforcement and ownership as Docker and remote monitoring have grown. Several critical policies are stronger in the documentation than in executable checks. Larger modules now combine state transitions, persistence and presentation. For a portfolio reader, focused extraction and trustworthy automation would improve the signal substantially.

The review applies `ce:architecting-systems` and `ce:writing-tests`, with the repository's rules taking precedence. Priorities below distinguish demonstrated enforcement defects from architectural improvements. Size and complexity limits are enforced guardrails: a limit decides when code must be split, and responsibilities and invariants decide where. Meeting a limit by cutting at line N, creating `pub(crate)` grab-bag modules or only moving inline tests out does not count.

This plan competes with the project's current stage. `PROGRESS.md` puts Kelvo in v1.x QA and polish with Release next, and three product gaps stay ahead of this plan whenever they conflict: Docker Desktop/Colima and native two-Mac acceptance are still open, the backgrounded tray measured 0.676% CPU with five Docker containers against the 0.5% target, and the Containers frontend gate runs at 31 to 35 ms/s against its 35 ms/s limit. Extractions in files with an open QA fix wait until that fix lands. Prefer existing tools and configuration over bespoke checkers; where this plan still proposes a custom check, it says why no existing tool covers it.

**1. Preserve agent autonomy while fixing ownership and destructive-action checks — high priority.**

Owner clarification: routine development should remain autonomous. This work must not introduce additional approval prompts for ordinary edits, scripts, builds, tests, dependency installation, non-destructive Git work, or cleanup of the task's own disposable outputs. Existing user authorization continues to apply. Optimize for fewer interruptions and reliable task completion.

[`auto-approve.sh`](../.claude/hooks/auto-approve.sh), lines 121–150, approves whole command families, including `env`, `git`, interpreters and development tools. The destructive-command scanner relies on matching particular command spellings. A classification-only probe of `git -C /tmp/kelvo-audit-example reset --hard` received no denial from `pre-bash.sh` and an explicit allow from `auto-approve.sh`. An `env python3 -c` command was classified as read-only. Neither command was executed.

Keep broad development-tool access and close three holes without narrowing any approved family. No command approved today should start prompting.

First, the destructive patterns in [`_destructive-patterns.sh`](../.claude/hooks/_destructive-patterns.sh), lines 214–244, match `git\s+reset`, `git\s+clean`, `git\s+push\s+--force` and similar, so `git -C dir reset --hard`, `git -c k=v clean -fd` and `git --git-dir=… push -f` slip through. Strip Git global options (`-C <path>`, `-c <k=v>`, `--git-dir=`, `--work-tree=`, `--no-pager`, `-P`) in that shared file before matching. The only behavior change is allow to deny, and only for commands already on the deny list.

Second, unwrap known wrappers instead of deferring them. For `env [opts] [VAR=val…]`, `timeout [opts] N`, `time`, `nohup`, `command` and `xargs [opts]`, strip the wrapper and classify the inner command with the same rules: approve it when the inner command would be approved alone, and defer only when the inner command is itself unknown. That keeps `timeout 120 bun run test` and `env RUST_LOG=debug cargo test` autonomous and removes the `env python3 -c` mismatch caused by `env` sitting on the read-only list at line 123. `bash -c`/`sh -c` and interpreter one-liners stay approved; the destructive scan already reads inside `-c` strings. Write down that these regex guards are a seatbelt against accidents rather than a security boundary: the backstops are the deny list, worktree isolation and Git history. Test the classifier without executing submitted commands.

Third, [`reap-orphans.sh`](../.claude/hooks/reap-orphans.sh), lines 41–43, builds its roots from `git worktree list`, so a Stop in one session kills orphaned Vitest or Playwright processes in sibling worktrees. A synthetic dry run selected a five-minute-old Vitest process in another checkout. Restrict the roots to the hook's own checkout (`$hook_root`, already computed at line 41), keep cleaning those automatically, and log matches in other worktrees to stderr without killing them. Per-task PID registration with process start identity is out of scope: hooks never receive the PIDs of Bash-tool children, so it would need a wrapper around every command, and PID reuse between one `ps` and the following `kill` is negligible.

[`format-changed.sh`](../.claude/hooks/format-changed.sh), line 15, runs `biome check --write` (a lint autofix, not only formatting) on every dirty tracked file, so it can rewrite another task's unfinished TypeScript. `cargo fmt --all` on a workspace that pre-commit keeps formatted is a no-op, so the TypeScript side is the real effect. [`lint-on-write.sh`](../.claude/hooks/lint-on-write.sh) already sees every file the agent writes; have it append each path to a per-session list keyed by the hook payload's `session_id`, and have the Stop hook fix only that list. This is low priority. Whole-workspace `bun run format` stays available.

Acceptance: positive fixtures prove routine development commands, common wrappers and same-checkout cleanup retain their autonomous behavior; negative fixtures cover destructive Git forms behind global options and wrappers, sibling-worktree processes, intentional background processes and preexisting dirty files. Use the existing `REAP_ORPHANS_*` test seams and classifier inputs. Compare decisions against the existing hooks so incidental syntax does not create new prompts. Run these tests in CI. No test sends a real destructive command or signal.

**2. Make CI cover the current workspace and define the trust of a green PR — high priority.**

The Linux job in [`ci.yml`](../.github/workflows/ci.yml), lines 309–316, enumerates five crates and omits both `kelvo-remote` and `kelvo-remote-helper`. [`Makefile`](../Makefile)'s `LINUX_CRATES` repeats the omission. Both are current workspace members, but D-032 and the platform-gating section of `.claude/rules/rust.md` define exactly five portable crates, so this is a scope change rather than an oversight. Add `kelvo-remote` (D-104 expects Linux reuse) and update D-032 and `rust.md` in the same change. Its tests spawn `/usr/bin/ssh-keygen` (`trust.rs:330`), so expect cfg gating or runner setup. `kelvo-remote-helper` ships inside Kelvo.app (`install.rs:398`); compile-check it on Linux if that is cheap, but Linux support for it is not a goal. Derive or verify the portable member list in one place so future additions cannot silently disappear.

The macOS job is manual, following D-096's cost decision for a private repository. D-096 names its own revisit trigger: the repository going public. `tryopendata/kelvo` is now public (verified with `gh repo view` on 2026-10-14), hosted macOS minutes are free for public repositories, and `ci.yml:20` still describes the repo as private. Restore one stable macOS leg on `pull_request` covering the app shell, native collectors, bindings freshness and packaging, and record a decision superseding D-096. Keep preview-OS and hardware/performance measurements separate. Inspect actual repository rules before claiming any check is required; this review inspected workflow source, not remote branch protection.

Remove `--pass-with-no-tests` from required Playwright runs. Add a production Vite build to automatic frontend CI; dev-server tests and `tsc` do not exercise production bundling. Put frontend performance checks in their existing single-worker lane rather than the full parallel browser run. Keep action SHA pins, restricted workflow permissions, frozen installs and cancellation of superseded runs.

Declare supported Bun/Node versions in repository tooling, align Node declarations with the supported runtime (CI currently uses Node 24 while `@types/node` is 26), and pin the Rust toolchain for reproducible review. Advisory dependency checks can remain a documented policy, but need an owner/remediation process rather than an indefinitely green warning lane. A schedule is optional and should be introduced only when its maintenance cadence is chosen.

Acceptance: a deliberate remote-crate compile failure and stale bindings fail their intended PR jobs; a zero-test browser run fails; a fresh checkout can use the documented versions and commands. Preserve the distinction between VM checks and real hardware evidence.

**3. Close the import-boundary holes with existing tools — high priority, demonstrated gaps.**

[`biome.json`](../biome.json) has useful import restrictions, but they mostly recognize strings. Temporary file probes confirmed that direct `react` and `~/app` imports from core fail, while these pass:

- `src/core` importing `../app/app`.
- `src/core` importing React-bound `create` from `zustand`.
- A widget importing runtime `commands` from `@core/generated/bindings`.

These are enforcement holes, not claims that current production code uses those bypasses. All three close in the existing Biome configuration. Add `../app/**`-style relative patterns to the core override. Add bare `zustand` to the core `paths`; today only `zustand/react` is blocked (`biome.json:80`). Use `importNames` on `@core/generated/bindings` to forbid `commands` and `events` in widgets while keeping types and constants. Turn the widget restriction into an allowlist: widgets currently import only `~/lib/utils` and `~/lib/motion/*`, so deny `~/**` with those two negations, which also stops indirect transport access through app hooks. Do not write a custom resolved-import checker. Dynamic imports and transitive reach are the only remaining gaps; adopt dependency-cruiser for them only if a real transitive violation turns up.

Check Cargo workspace edges against an explicit allowed graph too. Cargo prevents dependency cycles and undeclared imports; it cannot reject a newly declared dependency that violates the project's intended layering. A short `cargo metadata | jq` comparison in the Makefile is enough. The allowed graph is currently written down twice, in `plan/architecture.md` and `.claude/rules/rust.md`; the check reads one named source, and the other document points to it.

Fold in the three navigation calls flagged by Biome's promise rules (section 4) and review them here.

Acceptance: positive and negative Biome fixtures run in CI, including relative imports, bare Zustand, generated commands, legitimate generated types/constants and an app-hook import from a widget, plus an illegal Cargo edge. Existing production code passes the same rules.

**4. Tune TypeScript around actual runtime boundaries — low priority and opportunistic, except the route-parameter fix.**

[`tsconfig.json`](../tsconfig.json) already enables `strict`, `noUncheckedIndexedAccess`, `verbatimModuleSyntax`, unused-code checks and `noImplicitReturns`. Across 508 non-generated files under `src`, including colocated tests, AST inspection found zero explicit `any` keywords and zero non-null assertion expressions. Preserve those strengths.

The same project currently includes browser source, Vitest, Playwright and Node globals. Split a shared strict base into browser, test and tooling configurations. The browser compilation should not gain `process`, `Buffer`, `describe`, or `expect` merely because tests need them. Keep test and tooling declarations out of the application's ambient namespace; enforce Node built-in import restrictions as well, since `types` alone is not an import boundary. Type the supported Vite environment variables explicitly. Check every configuration in the existing `typecheck` command.

Adopt `exactOptionalPropertyTypes` deliberately and without urgency. Most patches cross Tauri as JSON, where `undefined` keys disappear anyway; the one concrete omission-versus-`undefined` hazard found is `containers/route.tsx:291-297`, which spreads a `Partial<DockerSettings>` patch over the current settings before `dockerConfigure`, so an explicit `undefined` would erase a setting. Fix that site directly regardless of the compiler flag. A compiler-API experiment enabling it together with `noPropertyAccessFromIndexSignature`, `noImplicitOverride` and `noUncheckedSideEffectImports` produced 101 diagnostics: 65 concerned optional-property assignability and 36 required index-signature bracket access. This is migration work, not evidence of 101 runtime bugs. Decide where omission differs from explicit `undefined`, especially settings patches and command options; permit `undefined` explicitly on React forwarding props when it is intentional. Do not silence the migration with casts or indiscriminate widening. TypeScript documents the distinction in [exactOptionalPropertyTypes](https://www.typescriptlang.org/tsconfig/exactOptionalPropertyTypes.html).

Prioritize environment isolation and optional-property meaning. Evaluate index-signature strictness separately: CSS-module and `dataset` access can create mechanical churn. Trial optional lint rules on the pinned tool version before enabling them. The installed Biome promise rules identified three navigation calls for review, while its exhaustive-switch rule falsely complained about variants already removed by an `in` guard in `remote-errors.ts`. Do not add incorrect switch cases to satisfy that diagnostic. Preserve compiler-backed exhaustiveness and the existing `never` checks.

[`containers/route.tsx`](../src/app/routes/dashboard/containers/route.tsx), lines 103–115, casts URL strings to unions and uses `rawRange in DOCKER_RANGES`. The latter accepts inherited names such as `toString` and `constructor`; evaluating the current expression produces `NaN` for the query start. The same `in` pattern appears at `detail-route.tsx:53` and `retained-history.tsx:77`, and neither of those guards `end` with `Number.isFinite` the way `route.tsx:113` does. Reachability is limited: the app uses `createMemoryRouter` with no URL bar (`router.tsx:107-114`), so malformed parameters can only come from the app's own navigation. Land `Object.hasOwn(DOCKER_RANGES, raw)` plus finite-timestamp checks at all three sites now as a small standalone fix (package P below), without waiting on any other package. The fuller route-state parser with normalized filters and location state belongs to the Containers controller work in section 7. Treat casts as assertions of proven invariants, never as parsing. Replace `as never` test fixtures with typed builders. Preserve localized indexed-buffer assertions where bounds are established and documented.

Use `satisfies` for finite configuration maps where retaining literal types helps, `readonly` for shared read inputs, discriminated unions for real lifecycle alternatives, and inference for local values. Avoid adding generic brands or elaborate type-level frameworks without a demonstrated misuse to prevent.

The current app/test TypeScript program checked in 3.80 seconds and reported about 529 MiB of compiler memory in one local run. This is a baseline, not a benchmark conclusion. After splitting environments, measure aggregate cold and warm checking time before adding project references or incremental caches. Keep `skipLibCheck` for ordinary iteration; evaluate declaration checking on dependency upgrades rather than imposing its cost blindly.

Acceptance: a negative type fixture using test/Node globals fails in the browser project; patch omission tests survive exact optional types; all configurations and generated consumers typecheck. The official [types documentation](https://www.typescriptlang.org/tsconfig/types) explains ambient inclusion.

**5. Separate transport contracts, native adapters and simulation — medium priority.**

[`transport.ts`](../src/core/transport.ts) combines a large interface, Tauri imports, command/event adaptation and query error helpers. [`mock-transport.ts`](../src/core/mock-transport.ts) is 1,918 lines, with streaming, settings, process selection and remote behavior sharing a large factory. This makes the seam harder to review as features expand.

The composition-root split partly exists already: `app-transport.ts:28` lazy-loads the mock. Keep one injected transport at the composition root, but split its implementation into contracts and feature-specific adapters. Move Tauri runtime access into the native adapter. Keep common result/query helpers independent of that runtime. Compose smaller mock modules for live streams, host setup, history and settings, extending the existing `mock/` organization. Narrow interfaces at consumers that only need a small capability; do not multiply React contexts solely to mirror interface names.

Generated types establish shape compatibility; they do not prove matching ordering, cancellation or failure semantics. Add shared protocol scenarios or Rust-produced fixtures for subscribe ordering, older backfill, cancellation before subscribe completion, disconnect, typed command failure and rejected transport promises. Keep the mock as a simulation, with deterministic time and explicit frame pushes. These protocol fixtures are valuable on their own and ship as a separate change (E2) from the mock-module split (E1).

Acceptance: moving an adapter does not change frontend behavior; the same recorded scenarios constrain Rust output and frontend simulation; `createMockTransport` and `createMockDocker` lose their size-gate exemptions (section 10). Hand-written contract and helper modules do not import the native adapter. That stops short of "no eager Tauri import" because `src/core/generated/bindings.ts:3-4` imports `@tauri-apps/api/core` and `/event` at the top level and at least ten core modules read runtime constants from it (`read-failure.ts`, `live-window.ts`, `brush.ts`, `live-state.ts`, `sampling-plans.ts`, `settings-patch.ts` and others). Removing that would need a codegen change that emits constants into a separate generated file; treat it as an optional follow-up. No hand edits to generated bindings.

**6. Restore clear ownership inside Rust application services — medium priority, architectural recommendation.**

[`docker/persistence.rs`](../crates/kelvo-engine/src/docker/persistence.rs) contains 4,212 lines, with tests beginning at line 2,612. Its production code combines ring admission, metadata, receipt/retry processing, memory accounting, retention, consistent read snapshots, historical projection and comparison ranking. It deserves the first substantial Rust extraction because those responsibilities have distinct invariants.

Retain `DockerRecorder` as a small coordinator. Extract ingestion/ring state, persistence acknowledgements and retries, snapshot lifetime/accounting, and history/comparison projection into private modules with explicit APIs. Preserve one clear owner for each invariant and document lock order. The size gate decides that this file must shrink; the responsibilities above decide the seams. Do not expose all state as `pub(crate)` to make extraction easy. Do the extraction as pure-move PRs with no logic edits, each merged quickly, so QA fixes to the recorder can land between them without conflicts. Recovery semantics repaired on 2026-10-13 (shutdown, pause, generation) are the invariants most at risk. Preserve bounded memory, real SQLite, generation/reset semantics, commit acknowledgements and cancellation deadlines.

[`src-tauri/src/remote.rs`](../src-tauri/src/remote.rs) combines Tauri commands, saved-host configuration, trust/setup orchestration, server adaptation, mirror ingestion and lifecycle management. [`history.rs`](../src-tauri/src/history.rs) combines store lifecycle/read pooling, query policy and IPC conversion. Extract reusable mirror/setup and query services into existing crates; leave Tauri handles, event emission, window effects and command translation in the shell. Keep app startup wiring in the composition root. These modules already contain useful webview-independent tests; move those tests with the services instead of replacing them with mocks. Before starting, name the target crate for each service and any new dependency edge it implies. `rust.md` treats an edge outside its list as a design change, so a new edge needs a decision entry first; prefer a placement that adds none.

Typed errors are a separate change (F2) from the extraction (F1), because they alter `CommandError`, the generated bindings, `dockerErrorMessage` and the mock transport, while the extraction must preserve behavior. Land F2 as one change across Rust, bindings and mock. Restore typed errors at meaningful boundaries. `DockerRecorder` query methods return `Result<_, String>` and the shell maps them into a single `CommandError::Docker { message }`. Remote `MirrorSink` also uses string errors. Introduce domain errors for invalid input, snapshot expiry/reset, resource limits, store failures and unavailable operations, preserving source errors. Map to IPC once so consumers can distinguish retry from correction or refusal without parsing text.

Acceptance: integration tests still use real SQLite and deterministic clocks; reset during read, acknowledgement failure, retry, bounded memory, cancellation and reconnect preserve their public outcomes. Native collectors, series identity and wire compatibility remain stable. The extracted functions and files lose their size-gate exemptions. Compare affected performance gates (`perf_gates` allocations and calls per tick) in the same sitting before claiming an improvement. Rollback rule: if a move-only extraction requires changing the expected output of any recovery, retry or bounded-memory test, revert it and reconsider the seam.

**7. Make React routes compose independently understandable behavior — medium priority.**

[`containers/route.tsx`](../src/app/routes/dashboard/containers/route.tsx) is 796 lines. It owns URL parsing, brush scope, membership hysteresis, stable color slots, previous chart paint, filtering/ranking, configuration mutations and most page markup. Several refs are mutated during render to preserve membership and paint history. Their correctness depends on coordination spread through one component.

Extract the validated route-state parser, a comparison controller for membership/focus/paint transitions, and separate activity/storage views. Define explicit transition inputs for engine, generation, range, metric and selection. A pure reducer can own state transitions; hooks handle effects and subscriptions; components receive presentation data. Keep Rust-owned aggregate and history semantics in Rust. Isolate the 1 Hz subscriptions at the leaves and preserve current selector and chart stability.

Move feature-specific helpers out of broadly shared components only when their consumers justify it. Introduce a reusable abstraction after a second real caller demonstrates the same semantics. Retain the existing render-only widget boundary and scoped stores.

Acceptance: changing engine cannot show old-engine data; range/metric changes reset the intended state; focus and colors remain stable; brush affects attribution without replacing the plot; unchanged cards do not render on unrelated frames. Exercise browser navigation/back and both activity/storage views. Use user-observable assertions and dedicated render-count tests. Run the Containers frontend perf gate in the same sitting against the pre-change build with no threshold change; it already measures 31 to 35 ms/s against a 35 ms/s limit, so a reducer or hook split that adds work per frame will show up there first. The route and its large components lose their size-gate exemptions.

Two test-hygiene items from section 9 ride along with this package: replace brittle class assertions in the touched components with rendered behavior, and switch the setup-step render-count test to React Profiler or an explicit observation seam.

**8. Tighten CSS exceptions while preserving chart performance — low priority, bounded cleanup.**

This section is app-wide (cards, base styles, metric, power, battery and zone components), so it is its own package (K) and does not ride with the Containers work. It has little user value; do it opportunistically when the touched component is already changing.

CSS is already compact: nine authored stylesheets totaling 578 lines; the largest is the 255-line theme. There is no authored `!important`. AST scanning found 123 JSX expression-style attributes across the non-generated `src` files, including tests and gallery. This is an inventory, not 123 violations. Most represent measured geometry, chart values, series colors, or CSS variables.

Apply the following policy consistently:

| Case | Preferred treatment | Current examples |
| --- | --- | --- |
| Static layout, spacing, color or opacity | Tailwind classes or SVG attributes | `metric-card.tsx:68` accent background; `power-stack.tsx:140` fill/opacity; constant `flexBasis: 0` in residency/stack bars; fixed `top: 2` in Timeline tooltip |
| Finite visual state | Explicit class map, variant or `data-*` selector | Charging stripe color in `battery-history-bars.tsx:93`; bounded appearance variants |
| Static geometry expressed through JS constants | One CSS token consumed by utilities, or a named utility when no shared calculation is needed | `zone-table.tsx` row/header heights; `process-selection.tsx` toolbar height; shared plot gutters |
| Continuously computed geometry | Inline value/custom property with a documented contract | Virtualized padding, measured overlay position, chart heights, heatmap cells, gauge fraction |
| Imperative high-frequency updates | Keep localized ref writes with lifecycle/performance tests | Brush/hover placement and streaming transform updates |
| Complex paint, keyframes, browser-specific controls | Co-located CSS Module; tokens/reset layers globally | Card gradient and animated custom property, search cancellation control, motion choreography |

Move static properties out of mixed static/dynamic style objects. Prefer SVG presentation attributes where appropriate. Do not duplicate a geometry constant in TS and CSS merely to remove an inline declaration; shared geometry needs one source of truth.

[`container-table.module.css`](../src/app/routes/dashboard/containers/_components/container-table.module.css) mainly expresses widths, display and truncation inside container queries. Tailwind v4 supports [container-query variants](https://tailwindcss.com/docs/responsive-design), so these rules can move into utility classes while preserving the existing breakpoints. Keep CSS when the utility representation becomes less understandable for genuinely structural behavior.

[`cards.css`](../src/app/styles/cards.css) retains global `.vt-card`/`.vt-card--chart` selectors despite the rule preferring CSS Modules and disallowing BEM variants. Move the component-owned effect into a module, with a data attribute for chart state; keep the registered property in its appropriate global layer. The gradient and custom-property animation warrant CSS. [`base.css`](../src/app/styles/base.css) mixes unlayered resets with `@layer base` and contains the two authored `@apply` uses. Put resets in the base layer and use semantic token declarations consistently, preserving native popover transparency and layer precedence. The popover-transparency rule `html[data-native][data-window="popover"]` (`base.css:14`) only applies in the native app, so a browser screenshot cannot verify this move; check the native popover with Reduce Transparency on and off, or leave `base.css` alone. Update the styling guidance where it incorrectly treats all pseudo-elements, media queries or `clamp()` as automatically requiring CSS; Tailwind can express many such cases.

Do not add a static-style checker. The inventory shows most inline styles are legitimate, and a checker with a documented-exception list is ongoing upkeep with no user value; keep the policy in `styling.md` and apply it in review. Do not blanket-ban `style`, or replace measured values with generated utility-class strings. Retain approved typography/arbitrary values from the design system rather than treating every bracketed class as abuse.

Acceptance: compare changed views with neighboring screens and the gallery in light/dark themes, reduced motion, performance mode and narrow layouts. Confirm brush hit testing and virtualization geometry. Run the existing performance lane for streaming changes; no visual or performance improvement is claimed by this source audit.

**9. Improve the evidence and the first-read experience — medium priority.**

Testing is a strength. Engine integration tests use fake time/collectors with real SQLite; remote session tests exercise real framed streams and storage. React tests generally use the transport boundary. Keep transport-call assertions where subscription demand or forbidden actions are themselves observable contracts. Do not mechanically replace all such tests or classify measurement-window waits as flaky sleeps.

Concentrate new tests on the demonstrated holes: architecture-rule fixtures, hook decisions, route parsing and production/mock contract parity, each of which belongs to its package above. Preserve intentional render-count tests. The class-assertion and setup-step render-count cleanups are assigned to package H.

The generated agent guidance is synchronized and its 17 tests pass. However, synchronized prose can still be stale. The top of [`PROGRESS.md`](PROGRESS.md) says the history constant is handwritten and no license is chosen; the current constant is re-exported from generated bindings and workspace/package manifests declare Apache-2.0. The main crate diagram and Rust rules omit the implemented remote/helper crates even though a later architecture section documents them. The motion rule table still describes a power-saver split while its later text describes all tokens being disabled in Performance mode. Reconcile current guidance with code and preserve historical entries as historical evidence.

`plan/README.md` already has "Architecture at a glance" (§8) and "Repo layout" (§11). Update those, or the root README, rather than adding a new document: cover the current dependency graph including the remote crates, one reading through collection → storage → transport → React, the mock/native distinction, test commands and important design tradeoffs. Link to the existing architecture and ADR documents for depth. Mark implemented and planned systems clearly. Avoid exposing future architecture as though it is already part of the executable system.

**10. Break up large files and functions, then prevent recurrence — high priority.**

Owner clarification: the refactor must address both oversized files and oversized functions throughout the codebase, with ongoing enforcement through Biome, Clippy and a Rust file-size check. Moving a large function into its own file does not complete this work.

The follow-up inventory confirms substantial targets. TypeScript counts below are physical declaration spans from the compiler AST, including nested functions, comments and JSX. Rust function counts are the installed Clippy lint's measurements, so the two columns are not directly comparable.

| Target | Observed size | Extraction boundaries |
| --- | --- | --- |
| `src/core/mock-transport.ts` / `createMockTransport` | 1,918-line file; 1,488-line function | Compose live-stream, history, settings, process and remote modules; keep one small transport factory |
| `src/core/mock/docker.ts` / `createMockDocker` | 833-line file; 544-line function | Scenario state, observation transitions, subscriptions and command handlers |
| Containers `LocalContainersRoute` | 796-line file; 719-line function | Validated route state, comparison controller, configuration actions, activity/storage views |
| Containers `ComparisonPlot`, `DockerStorage`, `ContainerTable` | Functions spanning 467, 443 and 423 lines | Chart geometry versus interaction; inventory versus selection/actions; table model, rows and virtualization |
| Timeline `LanesCard`, Hosts `AddMacDialog`, process `SelectionSession` | Functions spanning 372, 354 and 337 lines | Plot coordination versus overlays; setup state machine versus steps; selection transitions versus drag/keyboard interaction and UI |
| Docker recorder `record`, `comparison`, `history_page` | Clippy reports 237, 290 and 255 lines; owning file has 4,212 physical lines including tests | Observation/admission, ranking/projection, tier selection/merge; retain explicit snapshot and lock ownership |
| Collector Docker storage `classify` | Clippy reports 274 lines and cognitive complexity 27 | Resource-specific classification and accounting with a small composition function |
| Docker worker `run`, `session`, `cycle` | Clippy reports 134, 183 and 160 lines; `run` complexity 36 | Lifecycle transitions, retry decisions, one collection cycle and publication |

Extend the inventory across the workspace before implementation, including shell remote/history modules, store readers/writers, snapshot construction, collector normalization, seed/example code and gallery files. Rank by mixed responsibilities, branching and change frequency, using size as an additional signal. Separate production code from colocated tests in reports so a long integration-test module is not mistaken for a large runtime function. Tests and examples still need coherent organization.

Extract named operations with explicit inputs and outputs. Keep orchestration functions readable as a sequence of domain operations; move independent React sections into components and state transitions into pure functions/reducers where appropriate. Preserve private state and put helpers beside their owner. Avoid a generic `utils` directory, opaque context objects passed to every helper, excessive argument lists, numbered `part1`/`part2` modules, or splitting a transaction/lock lifetime across unclear boundaries. Do not compress formatting or remove useful comments to pass a line-count rule. Measure streaming render behavior and Rust allocation/call budgets where extraction could change them.

Proposed initial limits below are concrete implementation defaults. Calibrate once against representative well-factored code, then record the policy in canonical rules. Do not raise global limits to accommodate the existing outliers.

| Area | Enforcement | Initial limit |
| --- | --- | --- |
| TypeScript logic functions | Biome `complexity.noExcessiveLinesPerFunction` | 100 body lines, skipping blank lines |
| TSX functions/components | Same rule through the TSX override | 200 body lines, skipping blank lines; markup still gets extracted into coherent sections |
| TypeScript/TSX branching | Biome `complexity.noExcessiveCognitiveComplexity` | 15; applies independently of function length |
| Authored TS/TSX files | Biome `style.noExcessiveLinesPerFile` | 500 lines, skipping blank lines |
| Rust functions/methods | Clippy `too_many_lines`; `too-many-lines-threshold` in `clippy.toml` | 100 under Clippy's counting rules |
| Rust branching | Not gated initially; existing `too_many_arguments` stays on | Clippy's `cognitive_complexity` is a nursery lint that Clippy itself describes as of limited use. Run it as a report when choosing seams (it flagged `classify` at 27 and the worker `run` at 36), not as a gate |
| Authored Rust source files | Small repository script; Clippy has no file-length lint | 500 nonblank production lines, counted before the `#[cfg(test)]` module so moving tests out never counts as a split |

The Rust file script is the one custom check this section keeps, because neither Clippy nor rustfmt measures file length. It reads a plain-text list of current offenders with their ceilings, fails on any file over 500 production lines that is not listed or has grown past its ceiling, and fails when a listed file drops under the limit but is still listed.

The installed Biome (2.5.15) schema includes all three rules: `noExcessiveLinesPerFunction` and `noExcessiveCognitiveComplexity` in `complexity`, `noExcessiveLinesPerFile` in `style`. None is in the recommended preset, so each must be enabled explicitly. Configure `maxLines`, `skipBlankLines` and `maxAllowedComplexity` explicitly. The Rust workspace currently only declares `unwrap_used`; add the selected Clippy lints to `[workspace.lints.clippy]` and keep member lint inheritance. Avoid enabling all of `pedantic` or `restriction` as a substitute for choosing relevant rules. Clippy documents the [function-size and complexity configuration](https://doc.rust-lang.org/stable/clippy/lint_configuration.html); Biome documents [function length](https://next.biomejs.dev/linter/rules/no-excessive-lines-per-function/javascript/) and its [rule groups](https://biomejs.dev/linter/rules).

Use these rules for mocks as well as runtime code: a simulated backend is not exempt from maintainability. Exclude generated bindings and vendored upstream code by explicit path. Give tests and gallery/demo composition files a documented, separate profile: initially 1,000 nonblank lines per file and 200 lines per individual test/scenario function. A test-suite grouping callback may need a narrow exemption; individual test bodies and test helper logic should remain checked. Large declarative fixtures can be extracted into data files rather than suppressed as code. Production branching limits stay in force even in otherwise verbose JSX.

Introduce enforcement without making every unrelated change pay for the full backlog:

1. Turn every rule on at `error` (Biome) and through the existing `-D warnings` gate (Clippy, with `too_many_lines = "warn"` in `[workspace.lints.clippy]` and `too-many-lines-threshold` in `clippy.toml`) in one configuration change. Record the backlog with the tools' own suppression mechanisms rather than a custom baseline gate that parses lint output. For Rust functions, put `#[expect(clippy::too_many_lines, reason = "split tracked in package F1")]` on each offender; when the function drops under the limit the expectation goes unfulfilled, which warns and therefore fails under `-D warnings`, so stale exemptions remove themselves. For TypeScript functions, use an inline `// biome-ignore lint/complexity/noExcessiveLinesPerFunction: split tracked in H`; Biome reports suppressions that no longer suppress anything. For TypeScript files, add one Biome override per offending file with `maxLines` set to its current size rounded up to the next 50, so growth beyond that fails while small QA fixes can still land. Lower or delete the override in the same PR that splits the file.
2. Accept one limitation of the native approach: a function already carrying an exemption can grow without failing a check, since neither tool supports per-symbol thresholds. Every exempt function is already on the split list, and growth shows up in review. If that proves insufficient in practice, revisit a measurement-based gate then.
3. Keep exemptions out of the agent's way. `lint-on-write.sh` runs `biome lint` on every write and feeds errors back, so a live violation in an over-limit file would surface on every edit and push agents into splitting files during unrelated QA fixes. The suppressions and overrides above prevent that; do not leave known violations live.
4. Perform the extractions in work packages E, F1, G and H, plus the remaining collector/store targets above. Each completed target loses its exemption in the same PR. Avoid reducing cognitive complexity solely by hiding branches in helpers that still share all the same state.
5. Exempt test code narrowly: `#![cfg_attr(test, allow(clippy::too_many_lines))]` at crate roots and `#![allow(clippy::too_many_lines)]` atop integration test files (`--all-targets` still checks the non-test library build), and a Biome override applying the separate test/gallery profile above. Add positive and negative fixtures for the Rust file script, and one fixture per Biome rule proving the configured limit fires.

An exception needs a local explanation of the cohesive responsibility and why splitting it would hurt readability or a measured invariant. Agents can make routine, justified exceptions within the user's authorized scope; this policy introduces no new approval prompts. Exceptions must be visible in review and must not take the form of blanket crate/module-wide suppression. Preserve the autonomy requirement in section 1.

Other files over the limits that this plan does not name as targets, such as `kelvo-collect/src/docker_storage.rs`, `kelvo-engine/src/usage.rs`, `tray/render.rs`, `live.rs`, `usage-apps-card.tsx`, `process-selection.tsx` and `process-table.tsx`, keep their exemptions and get paid down when they are next substantially changed. Their overrides stop them growing in the meantime.

Acceptance: the named hotspots are decomposed into cohesive functions/modules; their exemptions are removed; new or growing violations fail locally and in CI; the resulting code passes behavior, type and affected performance checks. Smaller line counts alone are insufficient. Update `.claude/rules/rust.md`, `frontend/react.md`, `testing.md` and verification/command guidance as appropriate, then regenerate shared agent guidance with `make agent-config`. Record the size policy as a decision entry, since it is project-wide.

Follow-up diagnostics: the three Biome rules, trialed with their default settings across `src`, produced 281 informational findings (167 function-length, 38 file-length, 76 cognitive-complexity), including tests. These are an inventory, not 281 independent bugs or the count under the proposed limits. A targeted `cargo clippy -p kelvo-engine --lib -- -W clippy::too_many_lines -W clippy::cognitive_complexity` run completed and reported hotspots in the engine and its local dependencies. Neither experiment changed lint configuration.

**Suggested delivery order**

Sizes are rough single-maintainer estimates: S is under a day, M a few days, L a week or more.

| Work package | Size | Dependency | Reviewable result |
| --- | --- | --- | --- |
| P. Docker range-parameter fix | S | None | `Object.hasOwn` and finite-timestamp checks at the three sites, with regression tests; the `route.tsx:291-297` patch-spread fix |
| A. Hook holes without new prompts | S | None | Git global options normalized; wrappers unwrapped and recursed; reaping scoped to the hook's checkout; formatting scoped to session-written files; fixtures in CI |
| B. CI completeness and reproducible toolchains | M | None | macOS leg on PRs with a decision superseding D-096; `kelvo-remote` on Linux with D-032/`rust.md` updated; production frontend build; nonzero browser checks |
| J. Size/complexity guardrails | S | None | Biome and Clippy rules on, Rust file script, every current offender exempted with native suppressions; no behavior change |
| C. Architecture enforcement | S | None | Biome import rules closed, Cargo edge check against one named graph, fixtures in CI |
| I. Guidance reconciliation | S | Start now; finish after extractions | PROGRESS header, `rust.md`, `motion.md` and `plan/README.md` §8/§11 match the code |
| E1. Mock transport split | M | J | `createMockTransport` and `createMockDocker` decomposed; exemptions removed |
| E2. Protocol scenarios | M | None | Rust-produced fixtures for ordering, backfill, cancellation, disconnect and failures constrain the mock |
| F1. Docker recorder and collector extraction | L | B, J; entry condition below | Move-only PRs; recorder, worker and `classify` exemptions removed; recovery tests unchanged |
| F2. Typed Docker and mirror errors | M | F1 | Domain errors, new `CommandError` variants, regenerated bindings and mock in one change |
| G. Remote/history services | L | B, J; entry condition below | Target crates and edges named first; shell keeps Tauri concerns; exemptions removed |
| H. Containers controller and large components | L | J, P; entry condition below | Route, `ComparisonPlot`, `DockerStorage`, `ContainerTable` and the other named components decomposed; perf gate in the same sitting |
| D. TypeScript environments and exact optional types | M | C | Low priority: split ambient environments; optional-property semantics without escape casts |
| K. CSS policy cleanup | M | None | Low priority, opportunistic; native popover check if `base.css` changes |

E1 goes first among the splits because it is test infrastructure with no user-facing risk. F1, G and H share an entry condition: do not start a split on a file with a QA fix in flight, and land QA fixes first. Docker Desktop/Colima and two-Mac acceptance work takes precedence over F1 and G when they touch the same files.

Keep each package independently reviewable and behavior-preserving except for explicit correctness fixes (P, F2). Use scoped tests during development, complete the touched-side suites and type checks, and run the repository's final gates before committing/merging implementation. UI packages require screenshots and the gallery; schema/wire changes require an ADR and compatibility tests. No new crate-per-feature mandate, generic repository layer, event bus, frontend rewrite or storage-model redesign is needed for this plan.

**Review verification**

- `make agent-config-check agent-config-test`: current generated guidance; 17 tests passed.
- `bun run lint`: 566 files checked, no fixes or errors.
- `bun run typecheck`: both current configurations passed.
- Representative Vitest files for live-state reduction, host streaming and Containers: 47 tests passed across three files.
- `cargo test -p kelvo-remote --test sessions`: seven tests passed; ordinary Cargo through the retention proxy.
- Static import graph: 334 production non-generated source files, no cycles including type imports, and no core-to-app import edges in the scanned static imports/re-exports. Dynamic imports and transitive reach remain unchecked (section 3).
- Devil's-advocate review (2026-10-14) confirmed the cited file sizes, the CI and Makefile crate lists, the `DOCKER_RANGES` bug, the hook bypasses and the PROGRESS/rules drift by reading source; it corrected the `reap-orphans.sh` line citation (41–43) and found the two additional range sites. `gh repo view` reports `tryopendata/kelvo` as public, and `codex/disk-io-performance` is already merged into `main`, so it does not overlap E or G. The Rust size inventory used Clippy's own counts from a targeted run; a workspace-wide `cargo clippy -- -W clippy::too_many_lines` run to produce the exemption list has not been done.
- Temporary real-file Biome negative probes confirmed the bypasses; all probe files were removed. Hook probes used classification inputs and synthetic dry-run process data only. The optional compiler/linter experiments did not change configuration.

This is an architecture/source audit with representative checks. Full workspace suites, hosted CI execution, production bundling, screenshot comparisons, native two-Mac acceptance and performance benchmarks were not rerun. No application implementation changed. No task worktree was created; the primary checkout and its active caches are retained.


**Execution record (2026-10-14)**

Completed the owner-revised plan using `ce:executing-plans`, with isolated implementation
and a full-diff CE code review. The review narrative and delivery dependencies above are
preserved as the original audit, not a description of the completed code.

- [x] P — Validated Docker ranges/timestamps and omission-safe settings patches, with regressions.
- [x] A — Wrapper/global-option classification; checkout-owned reaping (including mixed paths); session-owned formatting. Routine command decisions remain autonomous. Deliberate exception to "no command approved today should start prompting": a wrapper now takes its inner command's classification, so wrapped forms of commands that prompt bare (`timeout 30 docker compose up -d`, `timeout 60 gh pr merge 12`, `env A=1 curl -X POST …`, `timeout 5 ssh host uptime`) prompt too; they were approved before only because the wrapper's name was. `rm` is approved as a utility, but any `rm` (bare, wrapped or quoted) whose target is `/`, `~`, `.`, `..`, `*`, starts with `$`/`~`, or is an absolute path outside the temp directories defers to a prompt. Post-review: Git global-option stripping covers any long option, and force pushes are caught wherever the flag or `+refspec` sits.
- [x] B — Stable macOS PR/push checks, Linux portable/remote coverage, production build and nonempty browser checks; pinned toolchains and dependency-audit policy.
- [x] J — Biome function/file/complexity gates, Clippy function length, tested Rust production-file ceilings; native, local legacy exceptions.
- [x] C — Tested Biome import boundaries and named Cargo dependency graph.
- [x] I — Current crate graph, reading path, runtime/test ownership and agent guidance; D-111 and D-112.
- [x] E1 — Feature-owned native adapters and mock services; small transport factories.
- [x] E2 — Rust-produced protocol fixture consumed by mock tests; subscription cancellation/disconnect and typed failures. Corrected the mock's inclusive history boundary.
- [x] F1 — Recorder ingestion/query/retention, Docker worker lifecycle/cycle and resource-specific collector classification; recovery behavior preserved.
- [x] F2 — Typed Docker and mirror errors with retained internal causes, typed IPC mapping and regenerated bindings. Mirror errors stay typed through `MirrorSink` and the session, and are flattened to `ConnectionStatus.history_error` text at publication, because the remote-host status IPC exposes only a sync state and a message and no consumer branches on the cause yet. Add a typed kind there when one does.
- [x] G — Engine-owned history lifecycle/queries, mirror epoch/recent/commit ownership and setup phases; no additional crate edges.
- [x] H — Containers controller/views and all named large components, including detail, comparison, storage, table, Timeline, host setup and process selection.
- [x] D — Browser, test and Node ambient environments; exact optional properties and explicit environment keys, without blanket casts.
- [x] K — Tailwind for static styling, component-owned card CSS module and retained justified dynamic geometry.
- [x] Final verification and independent code review. Commit, main integration and disposable-worktree cleanup are recorded in the task handoff.

Verification:

- `make check` plus the final frontend recheck: 916 Rust tests passed (34 ignored), 1,132 frontend tests in 179 files passed; workspace/all-targets Clippy, formatting, App Store compile checks, Biome and all three TypeScript projects passed.
- Architecture fixtures: 25 passed; generated-guidance fixtures: 17 passed. Hook fixtures: seven passed after review added mixed-checkout path coverage. Representative route, optional-patch, reader-generation, typed-error and subscription-race mutations failed their intended tests before restoration.
- Production build and all-files pre-commit checks passed. Text fixers now exclude binary CBOR fixtures and byte-preserved upstream font licenses. Generated bindings were regenerated from Rust; freshness is checked against the commit before integration.
- Chromium/WebKit functional suite: The complete browser matrix was exercised, with failure analysis and scoped rechecks described below. The earlier run had four failures during overlapping source/HMR activity; all four passed in isolation. A subsequent frozen run passed 402/404: pointer tracing reproduced a pre-existing WebKit row-focus scroll bug on both checkouts, which was fixed with unit and viewport-edge browser regressions. The other failure was a live-transition timing assertion; sixteen matched diagnostics and the final motion suite passed without changing motion code or assertions. All 48 affected Disk/process-selection/motion scenarios then passed; the new viewport-edge regression also passed in both browsers after failing on the original checkout.
- Frontend performance: nine of ten checks passed on the first lane; Containers initially measured 37.43 ms/s against 35. After CPU-profile diagnostics, sequential matched runs measured original 32.26 and refactor 32.46 ms/s with zero long tasks, passing the unchanged budget. No performance source change or speedup claim; the initial sample remains evidence of variability near the limit.
- Fixed-time light/dark Overview, Containers and gallery captures were byte-identical, including full gallery captures. Narrow and Performance-mode storage captures were identical; Containers differed only by tiny antialiasing variations (maximum channel delta three), with no layout change. Browser checks cover brushing, virtualized row geometry and the process-selection clip.
- Full-diff code review approved after fixing mixed-path orphan ownership and removing accidental Python bytecode artifacts. The later WebKit fix separates passive focus recording from keyboard-driven reveal; its incremental review also approved. No outstanding review findings.

Deliberate limits: 24 other Rust files retain explicit production-file ceilings, and
native function/file exceptions record the remaining backlog allowed by this plan.
Completed extraction targets lost their exceptions. `base.css` was left unchanged,
as permitted, rather than claiming unperformed native Reduce Transparency verification.
Hosted CI has been configured but not observed on a push; Linux execution, native
two-Mac/SSH acceptance, Docker Desktop/Colima acceptance and the existing background
CPU target remain separate open work. No whole-app CPU improvement is claimed.

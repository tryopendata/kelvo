---
name: frontend-design-slop
description: Recognize and avoid AI design slop in the Kelvo UI. Use when adding or restyling a dashboard section, popover layout, metric card, widget, heading, card grid, eyebrow/kicker label, stat tile, badge, or onboarding screen in src/app/, and when auditing existing routes for generic "made by an LLM" patterns. This is about visual and structural patterns, not prose.
---

# Frontend design slop

AI design slop is the visual fingerprint of an LLM that averaged instead of chose. Asked for a section, a model emits the modal answer from shadcn/Tailwind marketing pages: a tracked-out uppercase label, a heading, a subheading that paraphrases the heading, three identical rounded cards, a Sparkles icon, a gradient. None of it is ugly. All of it is recognizable, and readers have learned to read it as "nobody designed this."

The root cause is page-level, not component-level. A model re-derives each section independently, so every section gets the same anatomy and the same visual weight. There is no budget for emphasis. The fix is the same: decide once, per page, what matters most, and let only that section carry the weight.

Kelvo is a dense monitoring UI, so the specific failure here is less "marketing hero" and more "every module card gets the same icon-title-big-number-sparkline anatomy and the same weight", plus decoration that costs CPU at 1 Hz.

Read `plan/design-system.md` first and look at the screens already built next to the one you are changing. The design system is the spec: when it and this skill disagree, the design system wins. This skill covers what the design system is silent on: the structural and copy-in-UI patterns that stay generic even when every token is correct.

## The two tests that matter

**The deletion test.** Delete the element. If nothing is lost, it was decoration. Applies to eyebrows, subheadings, icons, badges, dots, and cards. Run it on every kicker and sub in a section header before shipping.

**The 25% zoom test.** Zoom the page out until you cannot read the text. If you cannot tell which section matters most, the page has no hierarchy. Every section has the same box, the same padding, the same header stack. Vary the container by the section's job: one is a full-bleed chart, one is a dense table, one is a single sentence and a link.

## Eyebrows, kickers, overlines

This is the pattern this codebase produces most. The tell is a three-line stack above every section: `KICKER` / heading / sub that restates the heading.

```
LIVE METRICS
CPU usage
See how busy your CPU is right now.
```

The kicker carries nothing the heading does not. The reader parses three lines to get one.

An eyebrow earns its place only when it adds a fact the heading lacks:

| Legitimate | Example |
|---|---|
| Category or taxonomy on a listing card | `Sensors`, `Efficiency cores` |
| Status or recency | `Paused`, `Last 24 h`, `Beta` |
| A real step in an ordered sequence the reader is following | `Step 2 of 4` |

An eyebrow is slop when it is any of:

- a paraphrase, synonym, or category-of-one for the heading directly below it (`PROCESSOR` over "CPU")
- present on every section of the page, so it stops being a signal
- a mood word or marketing phrase rather than a fact (`AT A GLANCE`, `UNDER THE HOOD`, `Why it matters`)
- numbered (`01`, `02`) when the sections are not steps the reader performs in order
- semantically a heading element (`<h2>`/`<h3>`). Eyebrows are `<span>` or `<p>`

The same test applies to the sub line. Keep it only when it adds a scope, a number, a constraint, or a caveat. "Real-time view of your processor" under "CPU" adds nothing. "10 cores · 4P + 6E" under "CPU" carries a fact; keep it only if the card body doesn't already show it.

Default for a section header in app UI: heading, optional one-line sub with real content, and an action slot. No kicker. If the section is one of many on a long page, use the heading's size and the section's container to signal weight, not a label.

## Where uppercase tracked text is fine

`text-[10px] uppercase tracking-wider text-muted-foreground` is not slop by itself. It is slop when it labels a section. It is fine when it labels a field:

- table column headers (process table, zone table)
- form group labels inside settings and the widget composer
- the mono uppercase label under or beside a figure in a metric card (`.data-mono` field labels)
- a sidebar group label in the dashboard nav (`Modules`, `History`)
- a compact status badge (`Throttled`, `On battery`)

The distinction: field labels sit next to a value or a control and name what it is. Section kickers sit above a heading and name what the heading already names.

## Catalog of patterns

Grouped so a reviewer can scan the codebase for them. The grep column is a starting point; every hit needs the deletion test, not a blanket rewrite.

### Typography and labeling

| Pattern | Looks like | Grep | Instead |
|---|---|---|---|
| Kicker above every h2 | `uppercase tracking-*` span immediately before an `<h2>` | `rg -B2 -A4 "uppercase" --glob '*.tsx'` and check for an adjacent heading | Drop it, or make it carry a fact |
| Sub that restates the heading | `<p class="text-muted-foreground">` under an h2 that says the same thing | read the pairs | Delete, or make it a constraint |
| Uppercase as decoration | `uppercase` on prose, nav labels, button text, section names | `rg "uppercase"` | Sentence case; weight and size for hierarchy |
| Numbered sections that are not steps | `01 / 02 / 03` mono pills on sections a reader does not perform in order | `rg "num=\"0"` | Remove the number unless it is a sequence |
| Isolated italic serif accent word in a headline | `font-serif italic` on one span | `rg "font-serif"` | Consistent hierarchy |

### Layout and structure

| Pattern | Looks like | Grep | Instead |
|---|---|---|---|
| Uniform section anatomy | Five to nine sections, each kicker + h2 + sub + grid | walk the page | Vary container by job; one dominant section |
| Three identical feature cards | `md:grid-cols-3` with three structurally identical children each with icon + title + blurb | `rg "grid-cols-3"` | A list, a bento with one dominant tile, a table, or two columns |
| Everything in a bordered rounded card | `rounded-xl border bg-white/[0.02] p-5` around content that is not independently actionable | `rg "rounded-xl border"` | Whitespace and proximity for grouping; cards only for units you can act on |
| Nested cards | a card inside a card inside a panel | read the tree | Flatten to one level |
| Centered hero + headline + two buttons | `text-center` + `text-5xl` + primary/ghost button pair | `rg "text-center" routes/*/hero*` | Asymmetry, a real product visual, one CTA |
| Big-number tile with no context | big figure + caption, no range, trend or comparison | `rg "tabular-nums" -B2 -A4` | Pair the figure with its sparkline, limit, or P/E split, as the existing module cards do |
| Mixed spacing scales | `p-3`, `p-7`, `mt-[37px]` on siblings | `rg "\[[0-9]+px\]"` | One scale per component family |

### Color and surface

| Pattern | Looks like | Grep | Instead |
|---|---|---|---|
| Gradient text, gradient borders, hero washes | `text-gradient`, `iridescent-border`, a full-bleed `bg-gradient-to-b` | `rg "text-gradient\|iridescent"` | Flat text and borders. The corner-glow accent card (9% tint, 6% on chart cards) is house style, not slop; leave it |
| Glow | `shadow-[0_0_8px]`, glowing accent dots | `rg "shadow-\[0_0"` | Depth from luminance steps |
| Accent color spread thin | module accent on eyebrows, dots, icons, links and badges in one card | `rg "text-primary"` per file | A card's accent marks its data series and nothing else; series use a lightness ramp of that one accent |
| Colored left border cycling through a palette | `border-l-4 border-emerald-500` per card | `rg "border-l-"` | Accent border only as consistent semantic status |
| Status color without a state machine | ad hoc `text-emerald-400` for "good" | `rg "text-emerald"` | Status colors only for real thresholds; emerald is also the Network accent and amber the Power accent, so warnings need icon + text |

### Iconography and decoration

| Pattern | Looks like | Grep | Instead |
|---|---|---|---|
| Sparkles or flourish icons | `<Sparkles>`, `<Zap>` on titles | `rg "Sparkles\|Zap"` | Name the thing |
| Icon-in-a-tinted-circle above every card title | `h-9 w-9 rounded-lg bg-primary/15` + lucide icon | `rg "place-items-center rounded-(md|lg) bg-"` | Icons only where they disambiguate |
| Accent dot before a label | `h-1.5 w-1.5 rounded-full bg-primary` | `rg "rounded-full\" aria-hidden"` | Nothing |
| Badge or pill on every heading | `rounded-full px-3 py-1 text-xs uppercase` next to an h1 | `rg "rounded-full.*uppercase"` | Badges for real state only |
| Emoji as icon | emoji literals in labels or headings | `rg -P "[\x{1F300}-\x{1FAFF}]"` | Icon set, or text |
| Arrow glyph after every link | `→` or `<ArrowRight>` on each card link | `rg "ArrowRight\|→"` | Links look like links |

### Motion

| Pattern | Looks like | Grep | Instead |
|---|---|---|---|
| Reveal on everything | entrance animations, staggered delays on cards | `rg "animate-\|delay-"` | Motion for continuity and feedback only (`.claude/rules/frontend/motion.md`) |
| Spring/bounce entrance | `spring: "bouncy"`, `bounce-in` | `rg "bounc"` | Never |
| Pulsing decorative dot | `animate-pulse` on a "live" indicator | `rg "animate-pulse"` | Remove. The numbers changing every second already say live, and an infinite animation costs idle CPU in a warm popover |
| Hover scale | `hover:scale-105` | `rg "hover:scale"` | Background luminance change |

### Copy in UI

| Pattern | Looks like | Instead |
|---|---|---|
| Weightless benefit words | Effortless, Seamless, Powerful, Unlock, Supercharge, Transform | A verb naming what happens |
| Formulaic heading | "Everything you need to…", "Your system at a glance", "How it works" | Name the module or the measurement |
| Mood-word kicker | AT A GLANCE, UNDER THE HOOD, LIVE | A fact, or nothing |

## Reviewing a page

1. Screenshot at 25% zoom. Name the section that matters most. If you cannot, fix hierarchy before touching labels.
2. Read only the h2s in order. They should read as an argument about this page's content, not a taxonomy that would fit any page.
3. For each section header: run the deletion test on the kicker, then on the sub. Keep what carries a fact.
4. Count cards. Module cards are the unit of the dashboard; cards around prose or around a single label are containers, not cards. Never nest them.
5. Count accent uses. Each card's accent marks its series. Anything else in accent is noise.
6. Count icons. Each one should disambiguate two things that would otherwise be confused.
7. Check the grid. Identical cards for modules with different shapes (cores vs. a single battery figure) means the content was fitted to a template. Ask what shape the data actually has, and compare against the screens already built.
8. Put a screenshot next to the neighboring screens and the dev gallery (`/?route=/dev/gallery`). Differences you can't justify are bugs.

## Related

- `plan/design-system.md` for tokens, module accents, type, surfaces, component inventory
- `src/app/routes/dev/gallery/` for every widget and component rendered on mock props
- `.claude/rules/frontend/styling.md` for Tailwind conventions
- `.claude/rules/frontend/motion.md` for motion and the idle-CPU budget
- `ce:design` plugin skill for the broader "avoid AI sameness" direction table

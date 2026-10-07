---
paths:
  - "src/**/*.css"
  - "src/app/components/**/*.tsx"
  - "src/app/routes/**/*.tsx"
  - "src/app/widgets/**/*.tsx"
---

# Styling Rules

Read `plan/design-system.md` before making visual changes, and look at the screens already
built that sit next to the one you are touching. Every feature is checked against both, by
screenshot in the browser dev server, before it is called done.

Before adding a section header, eyebrow/kicker label, card grid, stat tile, badge, or hero, load the `frontend-design-slop` skill (`.claude/skills/frontend-design-slop/SKILL.md`). It covers the generic "made by an LLM" patterns that stay generic even when every token is correct, and the deletion test every kicker and subheading has to pass.

## Stack

- **Tailwind CSS v4** with `@tailwindcss/vite` and `@theme` tokens
- **CSS Layers** (`@layer base, components, utilities`)
- **Semantic color tokens** via CSS custom properties
- **shadcn/ui** "new-york" components on Radix (copy-pasted, fully customizable), `cva` + `cn()`, lucide icons

## The styling ladder

Three tiers, in order. Reach for the next one only when the one above cannot express what you need.

1. **Tailwind utilities in the markup.** The default for spacing, colour, typography, layout, borders, radius and one-off tweaks. Semantic tokens (`bg-card`, `text-muted-foreground`), on-scale spacing.
2. **Custom CSS, scoped by the tool, not by naming.** For what utilities cannot express (the decision table below: keyframes, pseudo-elements, structural selectors, `clamp()`, dense media blocks). Use a co-located CSS Module, `thing.module.css`, imported as `styles` and applied as `className={styles.title}`. Vite hashes the names, so they can be short and local (`.title`, `.isOpen`). Plain stylesheets that must stay global wrap a component's rules in native `@scope (.block) { … }` and stay in the right `@layer`. No BEM. State and variants are `is-*` / `has-*` companion classes or `data-*` attributes; never encode state in the class name itself.
3. **Inline `style` only when the value comes from JS.** A measured width, a gauge fraction, a series colour from the accent ramp, a CSS custom property set from state. If the value is static, it belongs in tier 1 or 2.

## Design System Quick Reference

- Color tokens are semantic (`bg-background`, `text-foreground`, `border-border`), not hardcoded hex
- Dark mode is the primary design context
- Typography: self-hosted Inter Variable and JetBrains Mono Variable, OpenType features `"cv01", "ss03"`
- Figures use `.data-mono` (tabular, mono); field labels are mono uppercase
- Cards use the corner-glow accent card (9% tint; 6% on chart cards)
- Module accents: CPU cyan, GPU pink, Memory violet, Power & Sensors amber, Network emerald, Disk blue, Battery lime. Series inside a card use a lightness ramp of the card's accent, not other module accents
- Amber/red warning states must stay distinguishable from the amber Power accent: icon and text, never color alone
- Popover and widget surfaces over wallpaper use the vibrant surface tokens, with opaque fallbacks when Reduce Transparency is on

## 1 Hz rendering

Values change every second. Styling must not make that expensive or jittery:

- `tabular-nums` (or `.data-mono`) on every live number so widths don't shift each tick
- Fixed widths/min-widths on value slots; a value going from 9.9 to 10.0 must not reflow the card
- Animate `transform`/`opacity` only. Streaming charts scroll with `translateX`; never animate `width`, `height`, `top` or SVG path `d` per tick
- No CSS transitions on values that update every tick unless the transition is shorter than the tick and runs off a motion token, so Performance mode (`data-performance`, D-088) turns it off

## Animations

1. Prefer CSS animations for simple effects
2. Motion tokens come from `src/app/styles/motion.css` (per-tick vs one-shot, see `motion.md`), inside `prefers-reduced-motion: no-preference`
3. Always respect `prefers-reduced-motion` (see `motion.md`)

## CSS File Organization

**Default: style in the markup with Tailwind utility classes.** Reach for a `.css` file only for the things utilities genuinely can't express.

### When CSS, when utility

| You're styling…                                                              | Use                                  |
| ---------------------------------------------------------------------------- | ------------------------------------ |
| Spacing, color, typography, flexbox, basic grid, borders, radius, one-offs   | Tailwind utilities in `className`     |
| `@keyframes` and the rules that drive them                                   | Co-located `.module.css`              |
| Pseudo-elements (`::before`/`::after`), `::-webkit-*` form controls          | Co-located `.module.css`              |
| `nth-child`/`nth-last-child` structural logic, complex descendant selectors  | Co-located `.module.css`              |
| `clamp()`, `mask-image`, `clip-path`                                         | Co-located `.module.css`              |
| `@media`/`prefers-reduced-motion`/`prefers-reduced-transparency` blocks      | Co-located `.module.css`              |
| Theme tokens (`@theme`), global base resets                                  | the global theme stylesheet           |

### Avoiding god files

- **No single CSS file over ~400-500 lines.** Split it into partials imported in cascade order with explicit `@import` statements. No glob imports.
- **To dedupe a repeated utility string, extract a React component or a `.map()` loop, not `@apply`.**
- **Don't reach for `@apply` to "clean up" markup.** In a separate stylesheet prefer plain CSS variables (`background: var(--color-card)`). The legitimate `@apply` case is overriding a third-party library's styles (uPlot) while still using design tokens.
- **Don't create per-component CSS files** when a component should reach for utilities.

## Avoid

- Inline `style={}` props, **except** the sanctioned dynamic cases: CSS custom properties set from JS, values computed from props/state/data (gauge fractions, series colours, bar widths), and properties with no Tailwind utility.
- `!important`, **except** to override a third-party library's styles or inside a `prefers-reduced-motion` block.
- Color values outside theme tokens.
- **Off-scale spacing in className.** Prefer the scale: `p-4` not `p-[16px]`. Arbitrary values are an escape hatch, not the default.
- CSS-in-JS libraries (styled-components, Emotion).
- God CSS files and `@apply` used to dedupe markup (extract a component instead).

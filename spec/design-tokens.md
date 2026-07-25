# Tunnel Pilot v2 — Design Tokens

> Canonical design system for the Tauri v2 + Svelte rewrite. Every token below is
> paste-ready CSS custom properties. Agents: import this as the single source of
> truth. Do **not** hardcode hex/px anywhere in components — reference `var(--…)`.
>
> Continuity note: these tokens evolve v1 (Flutter `lib/app.dart`), not replace it.
> The confident blue brand accent is preserved; the palette is extended into a
> proper scale with elevation surfaces, status tints, and a focus ring so the
> Svelte build has everything v1 lacked. See `05-UI-UX-SPEC.md` for usage rules and
> `03-TECH-SPEC.md` for the IPC contract that feeds these surfaces.

---

## 1. How theming works

- Tokens live on `:root` (light, the default) and are overridden under
  `[data-theme="dark"]`. Theme is set by writing `data-theme` on `<html>`.
- A third mode `system` follows `prefers-color-scheme`; resolve it in JS to a
  concrete `data-theme` value so tokens never depend on the media query directly
  (keeps the 3-way theme picker honest and avoids flash on toggle).
- Semantic tokens (`--bg`, `--text`, `--accent`, …) are what components consume.
  The raw scale (`--blue-500`, …) exists only to derive semantics; components must
  not reference raw scale values directly.

```html
<!-- theme application -->
<html data-theme="light">   <!-- or "dark" -->
```

---

## 2. Color — raw scale (private, do not consume directly)

```css
:root {
  /* Brand blue (accent) scale */
  --blue-400: #4AA5E0;   /* dark-mode accent */
  --blue-500: #288DCC;   /* light-mode accent (brand anchor) */
  --blue-600: #147ABD;   /* accent-solid (fills w/ white text, AA-safe) */
  --blue-700: #0F6BA8;   /* accent hover (fills) */
  --blue-800: #0C5C90;   /* accent active (fills) */

  /* Neutral scale */
  --neutral-0:  #FFFFFF;
  --neutral-25: #F8F9FB;
  --neutral-50: #F1F3F6;
  --neutral-100:#E9ECF1;
  --neutral-200:#E2E5EA;
  --neutral-300:#CDD2DA;
  --neutral-400:#9CA3AF;
  --neutral-500:#6B7280;
  --neutral-600:#4B5563;
  --neutral-700:#374151;
  --neutral-800:#1A1D23;
  --neutral-900:#111318;

  /* Dark-mode neutrals (cool, slightly desaturated) */
  --ink-900: #0D0F13;
  --ink-850: #111318;
  --ink-800: #1A1D24;
  --ink-750: #22262F;
  --ink-700: #2A2F39;
  --ink-border: #2E333D;
  --ink-text:  #E5E7EB;
  --ink-text-2:#8B919A;
  --ink-text-3:#646B75;

  /* Status hues (base) */
  --green-500: #16A34A;  --green-400: #34D399;  --green-700: #15803D;
  --amber-500: #D97706;  --amber-400: #FBBF24;  --amber-700: #B45309;
  --red-500:   #DC2626;  --red-400:   #F87171;  --red-700:   #B91C1C;
}
```

---

## 3. Color — semantic tokens (consume these)

### Light (`:root`)

```css
:root {
  /* Backgrounds & surfaces (elevation increases lightness) */
  --bg:              #F8F9FB;   /* app canvas */
  --surface:         #FFFFFF;   /* cards, panels, dialogs */
  --surface-2:       #F1F3F6;   /* inset / nested / input fill on canvas */
  --surface-3:       #E9ECF1;   /* pressed inset, track backgrounds */
  --surface-overlay: #FFFFFF;   /* dialogs, command palette, menus */

  /* Text */
  --text:            #1A1D23;   /* primary — 15.0:1 on surface */
  --text-2:          #6B7280;   /* secondary — 4.8:1 on surface */
  --text-3:          #868C96;   /* tertiary/de-emphasized — ~3.0:1, non-essential only */
  --text-on-accent:  #FFFFFF;   /* text/icon on accent-solid fills */

  /* Borders & dividers */
  --border:          #E2E5EA;   /* default hairline */
  --border-strong:   #CDD2DA;   /* emphasized (focused card, active group) */
  --divider:         #E9ECF1;   /* list separators, subtler than border */

  /* Accent */
  --accent:          #288DCC;   /* brand: icons, links, selected accents, focus ring base (UI needs 3:1) */
  --accent-solid:    #147ABD;   /* filled button bg — white text = 4.62:1 (AA) */
  --accent-hover:    #0F6BA8;
  --accent-active:   #0C5C90;
  --accent-text:     #147ABD;   /* accent used as text on light bg (AA-safe) */
  --accent-subtle:   rgba(40,141,204,0.08);  /* selected-row tint / ghost hover */
  --accent-subtle-2: rgba(40,141,204,0.14);  /* selected-row tint, stronger */

  /* Interaction surface tints (theme-agnostic role, values differ per theme) */
  --hover:           rgba(0,0,0,0.04);   /* row/button hover tint */
  --active:          rgba(0,0,0,0.08);   /* pressed */

  /* Status — foreground (dot, icon, text on plain surface) */
  --status-connected:      #16A34A;
  --status-pending:        #D97706;
  --status-error:          #DC2626;
  --status-idle:           #9CA3AF;
  /* Status — subtle background tints (chips, banners) */
  --status-connected-bg:   #E7F6EC;   --status-connected-fg: #15803D;
  --status-pending-bg:     #FDF0DD;   --status-pending-fg:   #B45309;
  --status-error-bg:       #FDECEC;   --status-error-fg:     #B91C1C;
  --status-idle-bg:        #EEF0F3;   --status-idle-fg:      #4B5563;

  /* Focus ring (keyboard) */
  --focus-ring:      #288DCC;
  --focus-ring-halo: rgba(40,141,204,0.32);  /* outer glow */

  /* Scrim behind dialogs/palette */
  --scrim:           rgba(17,19,24,0.32);
}
```

### Dark (`[data-theme="dark"]`)

```css
[data-theme="dark"] {
  --bg:              #111318;
  --surface:         #1A1D24;
  --surface-2:       #22262F;   /* input fill, nested */
  --surface-3:       #2A2F39;   /* pressed inset */
  --surface-overlay: #1E222A;   /* dialogs/palette sit slightly above surface */

  --text:            #E5E7EB;   /* 12.6:1 on surface */
  --text-2:          #8B919A;   /* 5.9:1 on bg */
  --text-3:          #646B75;   /* ~3.3:1, non-essential only */
  --text-on-accent:  #0D1015;   /* near-black on bright accent — 6.9:1 */

  --border:          #2E333D;
  --border-strong:   #3A404B;
  --divider:         #22262F;

  --accent:          #4AA5E0;
  --accent-solid:    #4AA5E0;   /* dark text on bright accent = 6.9:1 (AA) */
  --accent-hover:    #63B4E8;
  --accent-active:   #7CC2EE;
  --accent-text:     #4AA5E0;   /* 4.9:1 on bg */
  --accent-subtle:   rgba(74,165,224,0.12);
  --accent-subtle-2: rgba(74,165,224,0.20);

  --hover:           rgba(255,255,255,0.05);
  --active:          rgba(255,255,255,0.09);

  --status-connected:      #34D399;
  --status-pending:        #FBBF24;
  --status-error:          #F87171;
  --status-idle:           #6B7280;
  --status-connected-bg:   rgba(52,211,153,0.14);  --status-connected-fg: #34D399;
  --status-pending-bg:     rgba(251,191,36,0.14);  --status-pending-fg:   #FBBF24;
  --status-error-bg:       rgba(248,113,113,0.14); --status-error-fg:     #F87171;
  --status-idle-bg:        rgba(139,145,154,0.14); --status-idle-fg:      #A2A8B2;

  --focus-ring:      #4AA5E0;
  --focus-ring-halo: rgba(74,165,224,0.40);

  --scrim:           rgba(0,0,0,0.55);
}
```

### Verified contrast (WCAG)

| Pair | Ratio | Verdict |
|---|---|---|
| `--text` #1A1D23 on `--surface` #FFF (light) | 15.0:1 | AAA |
| `--text-2` #6B7280 on #FFF (light) | 4.8:1 | AA (normal) |
| `--text-on-accent` #FFF on `--accent-solid` #147ABD (light) | 4.62:1 | AA (normal) |
| `--accent` #288DCC on #FFF (light) | 3.49:1 | AA for **UI/large only** — never body text |
| `--accent-text` #147ABD on #FFF (light) | 4.62:1 | AA (normal) — use for links |
| `--text` #E5E7EB on `--surface` #1A1D24 (dark) | 12.6:1 | AAA |
| `--text-2` #8B919A on `--bg` #111318 (dark) | 5.9:1 | AA (normal) |
| `--text-on-accent` #0D1015 on `--accent` #4AA5E0 (dark) | 6.9:1 | AA (normal) |
| `--accent-text` #4AA5E0 on `--bg` #111318 (dark) | 4.9:1 | AA (normal) |

`--text-3` intentionally sits ~3:1 — reserved for disabled controls, decorative
timestamps behind mono content, and never for information the user must read.

---

## 4. Typography

```css
:root {
  /* Cross-platform system UI stack (replaces v1 hardcoded ".SF Pro Text") */
  --font-ui: -apple-system, BlinkMacSystemFont, "Segoe UI", "Segoe UI Variable",
             Roboto, "Helvetica Neue", Arial, sans-serif;
  /* Monospace for technical/numeric content: ports, bytes, latency, timestamps, host:port */
  --font-mono: "SF Mono", "JetBrains Mono", Menlo, Consolas,
               "Liberation Mono", monospace;

  /* Type scale — named roles (size / line-height / weight) */
  --fs-title-lg: 18px;  --lh-title-lg: 24px;  --fw-title-lg: 700;
  --fs-title-md: 15px;  --lh-title-md: 20px;  --fw-title-md: 600;
  --fs-title-sm: 13px;  --lh-title-sm: 18px;  --fw-title-sm: 600;
  --fs-body-lg:  14px;  --lh-body-lg:  20px;  --fw-body:     400;
  --fs-body:     13px;  --lh-body:     18px;  /* default UI text */
  --fs-body-sm:  12px;  --lh-body-sm:  16px;
  --fs-label:    11px;  --lh-label:    14px;  --fw-label:    500;  /* uppercase, +0.4 tracking */
  --fs-mono:     12px;  --lh-mono:     16px;
  --fs-mono-sm:  11px;  --lh-mono-sm:  14px;

  --tracking-label: 0.4px;   /* section headers / overlines */
  --tracking-tight: -0.1px;  /* title-lg only */
}
```

Role → usage:
- `title-lg` — window/section page title (Connections, Settings). One per screen.
- `title-md` — dialog titles, group headers.
- `title-sm` — tunnel name, setting row label, card headings.
- `body` — the workhorse. Descriptions, menu items, form labels.
- `body-sm` — secondary detail, subtitles under settings toggles.
- `label` — UPPERCASE overlines: form group headers ("SSH SERVER"), section dividers.
- `mono` / `mono-sm` — all technical values. `host:port`, byte counts, latency ms,
  uptime, log timestamps, SSH command preview. Never set numeric/technical data in
  the UI font — the mono/proportional distinction is a core craft signal here.

Max **7** distinct type roles in the whole app. If a design needs an 8th, it's wrong.

---

## 5. Spacing (4 / 8 grid)

```css
:root {
  --sp-0:  0;
  --sp-1:  2px;
  --sp-2:  4px;
  --sp-3:  8px;
  --sp-4:  12px;
  --sp-5:  16px;
  --sp-6:  20px;
  --sp-7:  24px;
  --sp-8:  32px;
  --sp-9:  40px;
  --sp-10: 48px;
  --sp-11: 64px;
}
```

Rules: every margin/padding/gap uses a token. No arbitrary px (no `13px`, no `27px`).
Card internal padding `--sp-4`; between-card gap `--sp-3`; section gap `--sp-7`;
screen edge padding `--sp-5` (compact) / `--sp-6` (regular breakpoint).

---

## 6. Radii, borders, sizing

```css
:root {
  --radius-xs:   6px;   /* tags, chips, small pills */
  --radius-sm:   8px;   /* inputs, buttons, menu items */
  --radius-md:   10px;  /* cards, tunnel rows, group containers */
  --radius-lg:   12px;  /* dialogs, command palette */
  --radius-full: 999px; /* toggle track, status dot, avatar */

  --border-w:      1px;
  --border-w-emph: 1.5px;  /* focused input, active accent left-rail */

  /* Component sizing */
  --toggle-w: 36px;   --toggle-h: 20px;   --toggle-knob: 16px;  --toggle-pad: 2px;
  --input-h:  34px;                    /* single-line inputs, selects */
  --btn-h:    32px;   --btn-h-sm: 26px;
  --row-h:    56px;                    /* tunnel card min height (grows w/ stats) */
  --log-row-h:24px;                    /* dense log line */
  --titlebar-h: 40px;                  /* custom drag region */
  --tab-h:    38px;
  --hit-min:  28px;                    /* min interactive hit target (icon buttons) */
}
```

Radii preserve v1's language (8/10/12) and add `--radius-xs` for the new tags/chips
and `--radius-full` for the custom toggle + status dots.

---

## 7. Shadows / elevation

```css
:root { /* light */
  --shadow-1: 0 1px 2px rgba(17,19,24,0.06), 0 1px 1px rgba(17,19,24,0.04);   /* resting card (use sparingly) */
  --shadow-2: 0 4px 12px rgba(17,19,24,0.10), 0 1px 3px rgba(17,19,24,0.06);  /* menu, dropdown, drag proxy */
  --shadow-3: 0 12px 32px rgba(17,19,24,0.16), 0 4px 8px rgba(17,19,24,0.08); /* dialog, command palette */
}
[data-theme="dark"] {
  --shadow-1: 0 1px 2px rgba(0,0,0,0.40);
  --shadow-2: 0 6px 16px rgba(0,0,0,0.50), 0 1px 3px rgba(0,0,0,0.40);
  --shadow-3: 0 16px 40px rgba(0,0,0,0.60), 0 4px 10px rgba(0,0,0,0.45);
}
```

Elevation philosophy: **flat by default.** Cards/rows rely on `--surface` + `--border`,
not shadow. Shadow appears only when something genuinely floats: menus, drag proxy,
dialog, command palette, toast. This is the Linear/Raycast quietness — no shadow soup.

---

## 8. Z-index layers

```css
:root {
  --z-base:      0;
  --z-sticky:    10;    /* sticky toolbar / group header while scrolling */
  --z-dropdown:  100;   /* context menu, select popover, tag filter menu */
  --z-toast:     200;
  --z-scrim:     1000;  /* dialog/palette backdrop */
  --z-dialog:    1001;
  --z-palette:   1100;  /* command palette sits above dialogs */
  --z-tooltip:   1200;
}
```

---

## 9. Motion

```css
:root {
  --dur-instant: 80ms;    /* micro: dot color, checkmark */
  --dur-fast:    120ms;   /* hover tints, toggle knob, button press */
  --dur-base:    150ms;   /* tab/route crossfade (matches v1) */
  --dur-slow:    200ms;   /* dialog + command palette enter */
  --dur-reorder: 240ms;   /* list reorder settle */

  --ease-standard: cubic-bezier(0.2, 0, 0, 1);     /* default — Linear-like */
  --ease-decel:    cubic-bezier(0.05, 0.7, 0.1, 1);/* enter (dialogs, palette) */
  --ease-accel:    cubic-bezier(0.3, 0, 0.8, 0.15);/* exit */
  --ease-spring:   cubic-bezier(0.34, 1.4, 0.64, 1);/* toggle knob overshoot only */
}
```

```css
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 0.01ms !important;
    scroll-behavior: auto !important;
  }
}
```

Reduced-motion: cross-fades become instant swaps, drag proxy still follows the
cursor (position is not "motion" — it's direct manipulation) but the lift
scale/shadow animation is dropped. Status transitions snap. See motion spec in
`05-UI-UX-SPEC.md §Motion`.

---

## 10. Component tokens (quick reference)

| Component | Tokens |
|---|---|
| Toggle | 36×20 track, 16px knob, 2px inset, `--radius-full`; off=`--surface-3`, on=`--accent`, knob `--surface`; `--dur-fast` + `--ease-spring` |
| Input | height `--input-h` (34), `--radius-sm`, `--border` → focus `--border-w-emph` `--accent` + `--focus-ring-halo`; fill `--surface-2` in dark, transparent in light |
| Button (primary) | height `--btn-h`, `--radius-sm`, bg `--accent-solid`→`--accent-hover`→`--accent-active`, text `--text-on-accent` |
| Button (secondary) | `--surface`, `--border`, text `--text`; hover `--hover` |
| Button (ghost/icon) | transparent, hover `--hover`, `--radius-sm`, min `--hit-min` |
| Card / tunnel row | `--surface`, `--border`, `--radius-md`, padding `--sp-4`, min-height `--row-h` |
| StatusDot | 8px `--radius-full`, color = status token; connecting = pulse |
| StatChip | `--radius-xs`, `--surface-2` bg, mono-sm text, padding `--sp-1 --sp-2` |
| TagPill | `--radius-full`, `--surface-2`/tag-tint, body-sm, padding `2px 8px` |
| Dialog | `--surface-overlay`, `--radius-lg`, `--shadow-3`, max-width 460 |
| Command palette | `--surface-overlay`, `--radius-lg`, `--shadow-3`, width 560 |
| Focus ring | `outline: 2px solid var(--focus-ring); outline-offset: 2px;` + optional `box-shadow: 0 0 0 4px var(--focus-ring-halo)` |

Anything not covered: derive from the semantic tokens above. Do not invent new hex.

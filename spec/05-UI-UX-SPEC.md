# Tunnel Pilot v2 — UI/UX Specification

> Experience spec for the Tauri v2 + Svelte + TypeScript rewrite. Consumes the
> design system in `design-tokens.md`. The IPC command/event catalog is owned by
> `02-ARCHITECTURE.md` + `03-TECH-SPEC.md`; this doc references those commands
> abstractly (e.g. `tunnels.connect`, event `tunnel://stats`) — the exact names are
> authoritative there. Data model (ForwardConfig, groups, tags) is in
> `04-DATA-MODEL.md`. Scope: **v1 parity + command palette + resizable/responsive
> window + groups/tags + polish.**

---

## 0. Frontend ↔ backend contract (UI's view)

The UI is a thin, reactive render layer over Rust state. It never owns truth.

- **Reads (Svelte stores hydrated from IPC):** tunnel list + config, per-tunnel
  status + live stats, groups/tags, log buffer, app settings, update state.
- **Commands it invokes** (abstract — see `03-TECH-SPEC.md`): CRUD tunnel,
  duplicate, reorder; connect / disconnect / start-all / stop-all (optionally scoped
  to a group or tag); groups/tags CRUD + assign; settings get/set; backup
  export/import; update check/download/install; theme set.
- **Events it subscribes to** (push, exact names per `02-ARCHITECTURE.md`):
  `tunnel://status` (state machine transitions), `tunnel://stats` (emitted on the
  backend's single **3s** health/stats tick: active conns, bytes up/down, latency,
  uptime), `log://line` (new log entry), `log://cleared` (log buffer cleared),
  `update://progress`. Every event maps to a store update; the UI must render
  whatever the backend reports and never optimistically fake a "connected" state
  before the event confirms it (honest states, §Principles).

Contract rule: **optimistic UI only for reorder and local settings toggles.**
Connection state is always backend-confirmed — show `connecting` immediately on
intent, but only show `connected` on the event.

---

## 1. Design principles

1. **Keyboard-first, mouse-complete.** Every action reachable via keyboard
   (command palette + shortcuts + focus order). Mouse is never required. This is a
   power-user devtool that lives in the tray — treat it like a terminal, not a
   consumer app.
2. **Quiet until needed.** Flat surfaces, hairline borders, no shadow soup, no
   decorative color. Color and motion are earned by meaning (a green dot means
   connected; a pulse means connecting). Chrome recedes; data leads.
3. **Information density, done right.** Dense but not cramped — a 4/8 rhythm, mono
   for numerics so columns align, clear hierarchy. A user with 25 tunnels across
   3 environments should scan status in one glance.
4. **Honest states.** Never show success before it's real. `connecting` /
   `disconnecting` are first-class, not skipped. Errors say what failed and offer
   the next action (retry, edit, view log). Empty ≠ loading ≠ error — three
   distinct screens, never one grey blank.
5. **Continuity with v1.** Same brand blue, same radii language, same custom
   toggle, same Linear/Raycast restraint. A v1 user should feel upgraded, not
   relocated.

---

## 2. Information architecture & navigation

**Decision: sidebar rail, not top tabs.** v1 used 3 top tabs on a fixed 700×600
window. v2 is resizable and wider by default, so a **left icon+label sidebar rail**
replaces the tabs. Rationale:

- A resizable/wider window makes horizontal top-tabs waste the new vertical space;
  a rail scales and leaves the full content column for the dense tunnel list.
- The rail is where a persistent global search/⌘K affordance and the
  active-connection count live — always visible, unlike a buried tab.
- Rail collapses to icon-only at the compact breakpoint (see §3), preserving room.

Three destinations (same three as v1, renamed for clarity):

```
Connections   (default)   — the tunnel manager
Activity      (was Logs)   — reverse-chronological log stream
Settings                   — preferences, updates, backup/restore
```

Navigation is client-side routed (svelte routing or a simple store-driven view
switch) with a **150ms crossfade** (`--dur-base`) between destinations, matching v1.
Version number moves out of a per-tab footer into the Settings screen footer only
(it cluttered every v1 tab) — plus it's surfaced in the ⌘K palette ("About").

---

## 3. Window, layout & responsiveness

v1: fixed 700×600, non-resizable. v2: **resizable.**

- **Default size:** 860 × 640.
- **Min size:** 560 × 480 (enforced via Tauri `windowBuilder.minInnerSize`). Below
  this the toolbar + dense card content start to clip; we clamp instead.
- **Max:** unbounded; content column caps at 720px and centers within the content
  area past ~1100px so lines don't run absurdly wide (the list stays scannable).

### Breakpoints (content-area width, excludes rail)

| Name | Range | Rail | List/detail (v2.0) | Tunnel card |
|---|---|---|---|---|
| Compact | < 640 | icon-only (52px) | single column | stats wrap to 2 rows |
| Regular | 640–1000 | icon+label (176px) | single column | stats inline single row |
| Wide | > 1000 | icon+label | single column, list max 720, centered past ~1100 | stats inline |

- **v2.0 is list-only across all breakpoints.** Selecting a tunnel is a selection
  anchor (for Duplicate/Delete) and opens the **edit dialog** on `⏎` — there is no
  inline detail pane in v2.0. Wide simply gives the list more breathing room (caps
  at 720 and centers past ~1100 so lines never run absurdly wide).
- **Deferred to v2.1 (NOT required for v2.0):** a Wide-breakpoint right **detail
  pane** (min 320 / max 420) with full config read-out, a live bytes/s
  **sparkline**, and inline quick-actions. When built, selecting a tunnel at Wide
  opens this pane instead of the dialog; below Wide it stays a dialog. Backend
  moved the stat-history stream feeding the sparkline to Backlog v2.1+ as well —
  do not build against it in v2.0. The `Sparkline` component in §11 is likewise a
  v2.1 item.
- Responsive is CSS container-query / flex based; **verify at 560px** that nothing
  clips and the toolbar buttons collapse into a `⋯` overflow menu.

### Titlebar / drag / window chrome (Tauri v2) — **platform-split**

Window chrome is **not uniform** across platforms. macOS gets a custom transparent
titlebar (v1 continuity); Windows/Linux use **native OS decorations**.

- **macOS — custom transparent titlebar.** `titleBarStyle: Overlay` (transparent,
  full-size content). A `--titlebar-h` (40px) region at the top is the drag handle:
  mark it `data-tauri-drag-region`. Interactive children inside it (⌘K affordance)
  must set `data-tauri-drag-region="false"` so they stay clickable. Keep native
  **traffic-light buttons visible** (v1 shifted to visible in 1.4.x — continuity),
  inset via `trafficLightPosition`. Left-pad the rail header by ~76px so the rail
  title clears the lights. The app renders **no custom min/max/close** controls —
  the traffic lights own those.
- **Windows / Linux — native OS decorations.** `decorations: true`. The OS draws
  the real title bar (title text + minimize/maximize/close) and owns **drag, snap,
  double-click-maximize, and Aero/tiling behavior**. Do **not** render a custom
  titlebar, custom window controls, or a `data-tauri-drag-region` here — the app
  content starts directly below the OS chrome, and the rail header needs no
  traffic-light padding. The ⌘K affordance lives in the toolbar, not a titlebar.
- **Implication for the top drag-region wireframe below:** it depicts macOS. On
  Windows/Linux the "drag region (40px)" row is replaced by the native OS title bar
  and the content grid (rail + content) begins immediately under it.
- The app lives in the **tray** and hides-on-close on all platforms (parity).
  Close (native close button on Win/Linux, red traffic light on macOS) = hide
  window + keep running; quit is explicit (tray menu / ⌘Q). Backend owns tray.

```
┌───────────────────────────────────────── drag region (40px) ─────────────┐
│ ●●●  Tunnel Pilot                                    ⌘K  ⌕ Search…        │  (mac: traffic lights left)
├──────────┬────────────────────────────────────────────────────────────────┤
│  RAIL    │  CONTENT                                                         │
```

---

## 4. Screen: Connections

The core screen. Toolbar + grouped, reorderable tunnel list.

### 4.1 Toolbar (sticky, `--z-sticky`)

```
┌────────────────────────────────────────────────────────────────────────┐
│ Connections                          [+ Add]  ⧉  🗑   ⌕ Filter…   [tag ▾] │
│ 4 tunnels · 2 active                                                     │
└────────────────────────────────────────────────────────────────────────┘
```

- Left: `title-lg` "Connections" + a `body-sm --text-2` subtitle line
  "N tunnels · M active" (M active in `--status-connected` when > 0).
- Right cluster: **Add** (primary button), **Duplicate** + **Delete** (ghost icon
  buttons, disabled with tooltip when no selection), an inline **Filter** input
  (fuzzy filter the visible list — distinct from ⌘K global palette), and a
  **tag filter** dropdown. On Compact, Duplicate/Delete/tag collapse into `⋯`.
- Icons come from a real icon set (**Lucide** — matches Tauri/Svelte ecosystem and
  Raycast feel). **No emoji icons, ever.**

### 4.2 Groups / tags (NEW)

**Model (locked):** each tunnel belongs to exactly one **group** — a single
exclusive `groupId` acting like a folder (e.g. environment: dev / staging / prod) —
plus zero-or-more additive **tags** (free-form: `db`, `api`, `k8s`). Groups are
**flat, not nested**: no sub-groups, no group-within-group. A tunnel with no
`groupId` falls under the default "Ungrouped" section. Data model in
`04-DATA-MODEL.md`.

- **Group headers** are collapsible section rows within the list:

```
▾ PRODUCTION                                     2/3 active   [▶ Start all] [■ Stop all]
   … tunnel rows …
▸ STAGING                                        0/2 active   [▶ Start all]
```

  - Chevron toggles collapse. Collapsed state is **persisted on the group model**
    (`TunnelGroup.collapsed: bool`) and written via the `update_group` command — not
    a UI-only/ephemeral flag — so it survives restart. Header shows group name
    (`label`, uppercase, `--tracking-label`), an `X/Y active` count, and
    **Start all / Stop all** ghost buttons scoped to that group (calls the
    group-scoped connect/disconnect command). Stop all hidden when 0 active;
    Start all hidden when all active.
  - A thin `--border-w-emph` accent left-rail marks a group that has ≥1 active
    tunnel (quiet ambient status).
  - Ungrouped tunnels live under a default "UNGROUPED" header (no Start/Stop-all
    chrome if it's the only group; then headers are hidden entirely — a flat list
    for users who never adopt groups, so groups never tax the simple case).

- **Tags** render as `TagPill`s on the tunnel card (max 3 visible + "+N"). The
  toolbar **tag filter** dropdown lists all tags with counts; selecting one or more
  filters the list (AND/OR toggle inside the menu, default OR). Active filter shows
  as a removable pill in the toolbar: `[api ✕]`. Filter state is view-only (not
  persisted across sessions unless trivially cheap).

- **Assign** groups/tags in the forward form dialog (§7): a group `Select` and a
  tag multi-input (type-to-create, existing tags autocomplete).

- Edge cases: deleting a group offers "move tunnels to Ungrouped" vs "delete
  group only" (never orphan-delete tunnels). A tag with 0 tunnels is auto-pruned
  from the filter menu. Collapsed groups still count toward toolbar "M active".

### 4.3 Tunnel card (`ForwardCard`) anatomy

```
┌──────────────────────────────────────────────────────────────────────────┐
│ ● │ Postgres (prod)                          [db] [api]              ◯──● │   ← name, tags, toggle
│   │ 127.0.0.1:5432  →  10.0.4.12:5432                                     │   ← mono route
│   │ ⇅ 3 conns   ↑ 12.4 MB   ↓ 88.1 MB   ⟳ 41 ms   ◷ 2h 14m               │   ← stat chips (connected only)
└──────────────────────────────────────────────────────────────────────────┘
  ↑ status dot / accent left-rail when active
```

- **Status dot** (8px, left) + optional accent left-rail (`--border-w-emph`) when
  connected — the ambient "this is live" signal.
- **Name** (`title-sm`) + inline group hint only in filtered/flat views.
- **Tags**: right of name, `TagPill`s.
- **Route** (`mono`, `--text-2`): `local:port → remoteHost:port`. The arrow is a
  real glyph `→`, aligned; hosts truncate with middle-ellipsis, full value in
  tooltip + copyable.
- **Toggle** (right): the custom 36×20 toggle. Off = disconnected; On = connect
  intent. Toggling shows `connecting` immediately (see states).
- **Live stat chips** (connected only): active connections `⇅`, bytes up `↑`, bytes
  down `↓`, latency `⟳`, uptime `◷` — all `mono-sm` in `StatChip`s. Updated from the
  `tunnel://stats` event on the backend's **3s** tick, with a `--dur-fast`
  cross-fade on each value change (numbers ease between snapshots, never jump-flash).
  Because updates land every 3s (not continuously), treat each as a discrete
  snapshot: cross-fade the changed digits, do not run a per-frame counter/odometer
  animation between ticks. On Compact, wrap to 2 rows.
- **Hover:** row bg `--hover`, cursor default (row isn't a link; the toggle and
  context menu are the actions). **Selected** (click row body / arrow-key): bg
  `--accent-subtle` + accent left-rail. In v2.0 selection is a selection anchor for
  Duplicate/Delete, and `⏎` opens the edit dialog. (v2.1: at the Wide breakpoint
  selection opens the detail pane instead — §3.)
- **Context menu** (right-click or `⋯` on hover): Copy SSH command, Edit,
  Duplicate, Delete, Assign group ▸, Add tag ▸. Keyboard: `Menu` key or `⌥↵`.
  "Copy SSH command" writes to the system clipboard via
  `tauri-plugin-clipboard-manager` (not raw `navigator.clipboard`) → "Copied" toast.

### 4.4 Connection state machine (visual)

| State | Dot | Toggle | Card treatment |
|---|---|---|---|
| `disconnected` | `--status-idle` solid | off, knob left | plain surface, no stats |
| `connecting` | `--status-pending` **pulsing** (opacity 1↔0.4, 1s ease) | mid-travel, knob shows spinner ring | subtitle "Connecting…" replaces stats; toggle disabled to re-toggle for 300ms debounce |
| `connected` | `--status-connected` solid | on, knob right | accent left-rail, stat chips visible |
| `disconnecting` | `--status-pending` pulsing | mid-travel | "Disconnecting…" |
| `error` | `--status-error` solid | off | `--status-error-bg` inline strip under route: "Auth failed — <reason>" + `Retry` and `View log` links |

Never animate the dot color change abruptly — `--dur-instant` cross-fade. The
pulse is the *only* looping animation allowed on this screen, and only during
transitional states (respects reduced-motion → static amber dot + text label).

### 4.5 Reorder

Drag-to-reorder within a group (parity, improved). Drag handle appears on hover at
the card's left (⋮⋮ grip, `--text-3`), or whole-card drag on long-press. Lift proxy:
`scale(1.02)` + `--shadow-2` + slight opacity on the source gap (`--dur-fast`).
Drop settles `--dur-reorder` with `--ease-standard`. Reorder is optimistic
(the one place we don't wait for backend) then persisted via reorder command.
Dragging across group boundaries reassigns the group. Keyboard reorder: focus a
card, `⌥↑ / ⌥↓` moves it (announced via aria-live).

### 4.6 Empty & loading

- **Empty (no tunnels):** centered-in-content (this is the *one* justified centered
  layout — an empty state should be centered), a Lucide `plug-zap` line icon in
  `--text-3`, `title-md` "No tunnels yet", `body --text-2` one-liner, primary
  **Add your first tunnel** button, and a secondary "Import from backup" link.
  Not lorem — real copy.
- **Empty (filter/tag yields nothing):** "No tunnels match `api`" + Clear filter.
- **Loading (first hydrate):** skeleton of 3 card-shaped shimmer rows
  (`--surface-2` → `--surface-3` pulse, `--dur-slow`), rail + toolbar already
  painted. Should be near-instant (local state) — skeleton only if hydrate > 120ms.

---

## 5. Screen: Activity (Logs)

Reverse-chronological log stream (parity + polish).

```
┌────────────────────────────────────────────────────────────────────────┐
│ Activity                        [Level ▾]  ⌕ Filter…   [Copy all] [Clear] │
├────────────────────────────────────────────────────────────────────────┤
│ 14:22:07  INFO   postgres-prod   Tunnel established (41ms)               │
│ 14:22:06  WARN   redis-staging   Reconnect attempt 2/5                   │
│ 14:21:58  ERROR  api-gateway     Auth failed: permission denied          │
└────────────────────────────────────────────────────────────────────────┘
```

- Entire line is `mono-sm`. Columns align: `timestamp` (`--text-3`), `LEVEL`
  (colored badge-less token — INFO `--text-2`, WARN `--status-pending-fg`, ERROR
  `--status-error-fg`), `tunnel` (`--accent-text`), message (`--text`).
- **Level filter** dropdown (All / Info+ / Warn+ / Error). **Filter** input does
  substring match across tunnel+message. Both are view-only.
- **Row hover** `--hover`; **click a row = copy that line** (parity) → toast
  "Copied". **Copy all** / **Clear** ghost buttons. Copy-row and Copy-all write to
  the system clipboard via `tauri-plugin-clipboard-manager` (not raw
  `navigator.clipboard`). Clear needs no confirm (logs are ephemeral, max ~500 in
  backend buffer) but shows an undo toast for 4s.
- New lines stream in via the `log://line` event; a `log://cleared` event resets
  the view (e.g. after Clear). **Auto-scroll-to-top pinned** unless the user has
  scrolled down (then show a "N new" pill to jump back — no yanking the scroll out
  from under them).
- **Empty:** "No activity yet. Logs appear here when tunnels connect." — icon
  `scroll-text`.

---

## 6. Screen: Settings

Single scrollable column (max 640), grouped sections with `label` headers.

```
Settings
──────────────────────────────────────────
[ Update banner — see §8 ]

STARTUP
  Launch at login                                            ◯──●
  Show in Dock / taskbar                                     ●──◯

CONNECTIONS
  Desktop notifications                                      ◯──●
  Auto-reconnect            Retries: 5 · Delay: 3s           ◯──●
      └ (when on) Retries [ 5 ▾ ]   Delay [ 3s ▾ ]

UPDATES
  Automatically check for updates                            ◯──●
      └ Last checked 2h ago · [Check now]

APPEARANCE
  Theme      [ ☾ System ][ ☀ Light ][ ☾ Dark ]   ← segmented, 3 icons

BACKUP & RESTORE
  Export configuration                                    [ Export → ]
  Import configuration                                    [ Import ← ]
      └ Exports exclude passwords; identity-file paths are kept.

──────────────────────────────────────────
Tunnel Pilot v2.0.0 · check for updates
```

- Setting rows: `title-sm` label + optional `body-sm --text-2` subtitle, control
  right-aligned. Row height comfortable (`--sp-4` vertical), `--divider` between.
- **Theme** = 3-icon segmented control (parity), active segment `--accent-subtle-2`
  fill + `--accent-text`. Icons: System (monitor), Light (sun), Dark (moon) —
  Lucide.
- Auto-reconnect / auto-update sub-options **animate open** (`--dur-fast` height +
  fade) when their parent toggle is on; collapsed and non-focusable when off.
- **Backup rows are full-width clickable** (a v1.4.2 fix — keep them clickable, not
  just the trailing button). Export → native save dialog; Import ← native open
  dialog, then a confirm dialog summarizing "N tunnels will be imported / M will be
  overwritten" before applying.
- Footer: version (`mono-sm --text-3`) + "check for updates" link.

---

## 7. Forward form dialog (`ForwardFormDialog`)

Modal, `--radius-lg`, `--shadow-3`, width 460, scrim `--scrim`. Two sub-tabs
(General / Advanced) as an in-dialog segmented control; **state preserved** when
switching (parity — keep both tab DOM mounted / stores retained, don't remount).

```
┌─ Add tunnel ─────────────────────────────────────────  ✕ ┐
│  [ General ] [ Advanced ]                                 │
│                                                           │
│  Name          [ Postgres (prod)                       ]  │
│                                                           │
│  GROUP & TAGS                                             │
│  Group         [ Production                          ▾ ]  │
│  Tags          [ db ] [ api ]  [+ add tag…            ]   │
│                                                           │
│  SSH SERVER                                               │
│  Host          [ bastion.prod.internal               ]    │
│  Port  [ 22  ]      Username  [ deploy                ]   │
│                                                           │
│  AUTHENTICATION                                           │
│  ( ) Password        (•) Identity file                    │
│  Identity file [ ~/.ssh/id_ed25519            ] [Browse]  │
│                                                           │
│  PORT FORWARDING                                          │
│  Local  [127.0.0.1] : [5432]   →   Remote [10.0.4.12]:[5432]│
│                                                           │
│                              [ Cancel ]   [ Save tunnel ] │
└───────────────────────────────────────────────────────────┘
```

- Group headers are `label` uppercase overlines with `--sp-5` above.
- **Auth toggle** is a 2-option segmented / radio pair; switching swaps the field
  (password input vs identity-file picker). File picker defaults to `~/.ssh`
  (parity), returns via Tauri dialog plugin.
- **Advanced** sub-tab: keep-alive interval (seconds) + max unanswered count, each
  a labeled numeric input with `body-sm` help text.
- **Validation:** required (Name, Host, Username, local port, remote host/port);
  port range **1–65535**; live inline errors below the field in `--status-error-fg`
  `body-sm`, field border → `--status-error`. Save disabled until valid; on submit
  attempt with errors, focus first invalid field + shake-once (`--dur-fast`,
  reduced-motion → no shake, just focus).
- **Keyboard:** `Esc` cancels (confirm-discard only if dirty), `⌘↵` saves,
  `Tab` order top-to-bottom, focus trap within dialog, focus returns to the trigger
  on close.
- **Edit mode:** title "Edit tunnel", Save label "Save changes", disabled until a
  field changes (dirty tracking).

---

## 8. Update banner (Settings, top)

Multi-state (parity), a single inline banner (not a toast) with `--radius-md`.

| State | Treatment |
|---|---|
| `idle` (up to date) | hidden, or a quiet `--text-2` line "You're on the latest version." |
| `available` | `--accent-subtle` bg, "Version X available" + release-notes disclosure + **Download** button |
| `downloading` | progress bar (`--accent`), "Downloading… 42%" (`mono`), Cancel |
| `installing` | indeterminate bar, "Installing…" (no cancel) |
| `ready` | `--status-connected-bg`, "Update ready" + **Restart to update** |
| `error` | `--status-error-bg`, "Update failed: <reason>" + **Retry** + View log |

Progress driven by the `update://progress` event. Banner never auto-dismisses in `ready`
or `error` — user must act. Existing self-update subsystem behavior is in the
architecture docs; UI only renders these six states.

---

## 9. Delete confirmation dialog

Small dialog (width 380). Title "Delete tunnel?" `title-md`. Body: "**<name>** will
be permanently removed. This can't be undone." (real name interpolated). If the
tunnel is connected, add a `--status-pending-fg` line "This tunnel is currently
connected and will be disconnected." Buttons: **Cancel** (secondary, default focus)
+ **Delete** (destructive — `--status-error` fill, white text, ≥4.5:1). Multi-delete
variant: "Delete N tunnels?" with a scrollable name list. `Esc` = cancel,
`↵` = the *safe* action (Cancel), never auto-confirm destruction.

---

## 10. Command palette (NEW) — `⌘K` / `Ctrl+K`

Keyboard-first fuzzy launcher, the marquee v2 feature. Raycast-grade.

```
              ┌──────────────────────────────────────────────────┐
              │ ⌕  connect prod db|                                │  ← input, mono-ish
              ├──────────────────────────────────────────────────┤
              │  TUNNELS                                          │
              │  ● Postgres (prod)      127.0.0.1:5432   Connect →│  ← highlighted
              │  ○ Redis (prod)         127.0.0.1:6379   Connect  │
              │  ACTIONS                                          │
              │  ⇅ Start all in Production                        │
              │  ⏻ Stop all tunnels                               │
              │  ⊕ Add tunnel                            ⌘N       │
              │  ☾ Toggle theme                                   │
              │  ⚙ Open Settings                         ⌘,       │
              ├──────────────────────────────────────────────────┤
              │  ↑↓ navigate   ↵ run   ⌘↵ secondary   esc close   │  ← footer hints
              └──────────────────────────────────────────────────┘
```

- **Invoke:** `⌘K` (mac) / `Ctrl+K` (win/linux) from anywhere, even while a dialog
  is open (palette sits at `--z-palette`, above dialogs). Also a visible `⌘K`
  affordance in the titlebar.
- **Layout:** centered horizontally, offset from top (~15vh), width 560, `--radius-lg`,
  `--shadow-3`, scrim behind. Enter: fade + `translateY(-8px)→0` scale `0.98→1`
  over `--dur-slow` `--ease-decel`. Exit `--dur-fast` `--ease-accel`.
- **Search:** fuzzy across (a) tunnel names + host + ports, (b) static actions.
  Results grouped by section (TUNNELS, ACTIONS) with `label` headers. Ranking:
  exact/prefix > subsequence; recently-used actions float up.
- **Per-tunnel result** shows status dot + name + `mono` route; primary action is
  context-aware — **Connect** if disconnected, **Disconnect** if connected —
  labeled on the right. `⌘↵` runs the *secondary* (Edit). `→` opens a sub-menu of
  all actions for that tunnel (connect/disconnect/edit/duplicate/delete/copy cmd).
- **Global actions:** connect/disconnect specific, Start all / Stop all (global or
  per group), Add tunnel (`⌘N`), Open Settings (`⌘,`), Toggle theme, Check for
  updates, Go to Activity, About/version.
- **Keyboard:** `↑/↓` move (wraps), `↵` run primary, `⌘↵` secondary, `→` sub-menu /
  `←` back, `esc` closes (or backs out of sub-menu first). Typing always refocuses
  input. Mouse hover also moves the selection (single active model — hover and
  keyboard share one highlighted index).
- **Empty query:** show recents + top actions, not a blank void. **No match:**
  "No results for `xyz`" + a hint to `⌘N` add a tunnel.
- **A11y:** `role="dialog" aria-modal`, input is a `combobox` with
  `aria-activedescendant` pointing at the highlighted option; results are
  `role="listbox"`/`option`. Announce section + result count via aria-live.

---

## 11. Component inventory

Reusable Svelte components. Props are the contract; every listed state must exist.

| Component | Key props | States / notes |
|---|---|---|
| `Button` | `variant: primary\|secondary\|ghost\|danger`, `size: sm\|md`, `loading`, `disabled`, `iconLeft`, `iconOnly` | hover/focus-visible/active/disabled/loading(spinner, label retained) |
| `Toggle` | `checked`, `disabled`, `ariaLabel` | off/on/hover/focus-visible/disabled; 36×20, knob spring |
| `Input` | `value`, `type`, `placeholder`, `invalid`, `errorText`, `mono`, `prefix/suffix` | default/hover/focus(1.5px accent+halo)/invalid/disabled |
| `NumberInput` | + `min`,`max`,`step` | port-range validation baked; mono |
| `Select` | `options`, `value`, `placeholder` | closed/open(popover `--z-dropdown`)/focus/disabled; keyboard type-ahead |
| `SegmentedControl` | `options`, `value` | used for theme + form sub-tabs + auth; active `--accent-subtle-2` |
| `Card` / `ForwardCard` | `tunnel`, `status`, `stats`, `selected` | full state machine §4.4; hover/selected/error strip |
| `StatusDot` | `status` | idle/pending(pulse)/connected/error; 8px |
| `StatChip` | `icon`, `value`, `mono` | value-change cross-fade |
| `TagPill` | `label`, `removable`, `tone` | default/hover/removable(✕)/active-filter |
| `GroupHeader` | `name`, `activeCount`, `total`, `collapsed` | collapsed/expanded; Start/Stop-all buttons; active accent rail |
| `SidebarItem` | `icon`, `label`, `active`, `badge` | rest/hover/active(accent rail + `--accent-subtle`)/icon-only(compact) |
| `Dialog` | `title`, `size`, `onClose` | scrim, focus trap, `esc`, return focus; enter/exit motion |
| `CommandPalette` | (singleton) | see §10 |
| `Toast` | `message`, `tone`, `action?`, `duration` | enter/exit slide+fade top-right; stack; `--z-toast` |
| `EmptyState` | `icon`, `title`, `body`, `action` | one per empty surface; real copy |
| `Skeleton` | `variant: card\|row\|line` | shimmer, reduced-motion → static |
| `Tooltip` | `content`, `delay` | keyboard-focus + hover trigger; `--z-tooltip` |
| `ContextMenu` | `items` | right-click + keyboard; `--z-dropdown` |
| `ProgressBar` | `value?` (indeterminate if null) | determinate/indeterminate |
| `Sparkline` **(v2.1)** | `series` | detail-pane bytes/s; canvas/svg, `--accent` line, no axis chrome. Deferred with the Wide detail pane — not in v2.0 |

---

## 12. States matrix (per surface)

| Surface | Empty | Loading | Error | Offline / edge |
|---|---|---|---|---|
| Connections | §4.6 first-run CTA | 3-row skeleton (>120ms) | per-card error strip; global "N tunnels failed" toast | n/a (local) |
| Filter/tag result | "No tunnels match X" + clear | — | — | — |
| Activity | "No activity yet" | instant (in-memory) | — | — |
| Settings | n/a | instant | setting write fail → toast + revert control | — |
| Update banner | idle/hidden | downloading/installing bars | error state + Retry | no network → "Couldn't reach update server" |
| Command palette | recents + top actions | — | "No results for X" | — |
| Detail pane *(v2.1)* | "Select a tunnel" placeholder | — | — | — |

**Honest-state rule restated:** a surface must visibly distinguish *empty*
(nothing exists), *loading* (fetching), and *error* (failed). Never collapse them
into one grey blank — that's the laziest slop tell.

---

## 13. Motion spec

| What | Duration / easing | When NOT to animate |
|---|---|---|
| Route/section crossfade | `--dur-base` standard | — |
| Hover tint (rows, buttons) | `--dur-fast` standard | reduced-motion → instant |
| Toggle knob | `--dur-fast` `--ease-spring` | reduced-motion → snap, no overshoot |
| Dialog / palette enter | `--dur-slow` `--ease-decel` (fade + slight rise/scale) | reduced-motion → instant appear |
| Dialog / palette exit | `--dur-fast` `--ease-accel` | " |
| Connecting/disconnecting pulse | 1s ease loop on dot | reduced-motion → static dot + text |
| Stat value change | `--dur-fast` cross-fade | never flash/blink numbers |
| Reorder lift + settle | `--dur-fast` lift, `--dur-reorder` settle | reduced-motion → no scale/shadow, position only |
| Sub-option expand (settings) | `--dur-fast` height+fade | reduced-motion → instant |
| Skeleton shimmer | `--dur-slow` loop | reduced-motion → static block |

**Never animate:** status *color* changes beyond `--dur-instant`; anything
looping except the two transitional pulses/shimmer; page-load "reveal on scroll"
theatrics (this is a utility, not a landing page); no parallax, no decorative
entrance staggers on the tunnel list.

---

## 14. Accessibility

- **Focus order:** titlebar controls → rail items → toolbar → list (group header →
  cards, each card: row → toggle → menu) → footer. Logical, top-to-bottom,
  left-to-right. Focus trap in dialogs + palette; focus returns to trigger on close.
- **Focus visible:** every interactive element gets the `--focus-ring` outline (2px
  + halo). Never remove outlines without a replacement. Mouse users don't see it
  (`:focus-visible`), keyboard users always do.
- **Keyboard:** entire app operable without mouse (§15). Reorder via `⌥↑/↓`.
  Context menu via `Menu` key / `⌥↵`.
- **ARIA / semantics:** real semantic elements — `<button>` for actions, `<nav>`
  for rail, `<main>` for content, `role="listbox"` for palette results, `<dialog>`
  or `role="dialog" aria-modal`. Toggle = `role="switch" aria-checked`. Status dot
  has `aria-label` ("Connected") + text isn't color-only (state label present in
  the accessible name / stat area).
- **Color independence:** status is never conveyed by color alone — dot shape/label,
  the toggle position, and the stat presence all reinforce it. Log LEVEL is text,
  not just color.
- **Contrast:** all text meets AA per `design-tokens.md §3`; `--text-3` reserved for
  non-essential only.
- **Hit targets:** interactive elements ≥ `--hit-min` (28px) effective target even
  when the visual glyph is smaller (pad the click area).
- **Reduced motion:** honored globally (§13); the app remains fully legible and all
  state changes remain perceptible without animation.
- **Screen reader announcements** (aria-live polite): connection state changes
  ("Postgres prod connected"), reorder moves, copy confirmations, import result.

---

## 15. Keyboard shortcut map

**Global**

| Keys | Action |
|---|---|
| `⌘K` / `Ctrl+K` | Command palette |
| `⌘N` / `Ctrl+N` | Add tunnel |
| `⌘,` / `Ctrl+,` | Settings |
| `⌘1 / ⌘2 / ⌘3` | Connections / Activity / Settings |
| `⌘F` / `Ctrl+F` | Focus in-view filter (Connections/Activity) |
| `⌘⇧⏎` | Start all · `⌘⇧⌫` Stop all (global) |
| `⌘W` | Hide window (to tray) · `⌘Q` Quit |

**Connections (list focused)**

| Keys | Action |
|---|---|
| `↑ / ↓` | Move selection · `⌥↑ / ⌥↓` reorder |
| `Space` | Toggle connect/disconnect selected |
| `⏎` | Edit selected tunnel (opens dialog; v2.1: detail pane at Wide) |
| `⌘D` | Duplicate · `⌫` Delete (→ confirm) |
| `⌘C` | Copy SSH command of selected |
| `←/→` | Collapse / expand focused group |

**Dialogs / palette**

| Keys | Action |
|---|---|
| `Esc` | Cancel / close (back out sub-menu first) |
| `⌘⏎` | Save (form) / secondary action (palette) |
| `Tab / ⇧Tab` | Field navigation (trapped) |

Shortcuts are discoverable: shown in the ⌘K palette next to each action and in
tooltips. Provide a "Keyboard shortcuts" entry in the palette (`?`).

---

## 16. AI-slop pitfalls to avoid (app-specific)

This is a dense keyboard-driven devtool. The following would make it read as
generated-by-default — do **not** do them; do the crafted alternative instead.

| Slop pitfall | Why it's wrong here | Crafted alternative |
|---|---|---|
| Generic 3-column card grid of tunnels | Tunnels are a scannable *list* with status; a grid destroys vertical scanning + wastes width | Single-column dense list, grouped, mono-aligned routes |
| Meaningless gradients (purple→blue hero, gradient buttons) | No hero here; gradients add zero meaning to a status tool | Flat `--surface` + hairline `--border`; color only for status/accent |
| Emoji as icons (🚀🔒⚡) | Inconsistent rendering, unprofessional in a devtool | Lucide line-icon set, one weight, `currentColor` |
| Everything centered | A data list centered in whitespace kills density | Left-aligned list; centering reserved only for empty states |
| Glassmorphism / blur blobs | Decorative, hurts text contrast, off-brand | Solid surfaces, real elevation only when floating |
| Arbitrary px (13px, 27px paddings) | The v1 audit already flagged scattered magic numbers | Everything on the `--sp-*` 4/8 scale |
| Too many type sizes/weights | Muddies hierarchy | Max 7 roles (`design-tokens.md §4`) |
| Numerics in UI font | Ports/bytes/latency misalign, look amateur | `--font-mono` for all technical/numeric values |
| Skipping states (only happy path) | v1's own weakness was thin states | Full state machine §4.4 + states matrix §12 |
| Faked success before backend confirms | Dishonest; hides real failures | Backend-confirmed `connected` only (§0) |
| Motion everywhere / reveal-on-scroll | This is a utility, not a marketing page | Purposeful micro-motion only (§13) |

A design here is judged crafted when: status is glanceable, everything aligns to
the grid, numerics are mono, every state is honest, and the whole thing is driveable
from the keyboard. Restraint is the brand.

---

## 17. Self-review — Slop Score of this spec's target design: **1/10**

Crafted, not slop. The system has intent at every layer: a real 4/8 spacing scale,
7 capped type roles, semantic color with verified AA contrast, one radius/shadow
language carried from v1, a genuine state machine, honest empty/loading/error
separation, and keyboard-first operation. The previously-deducted point (Wide detail
pane + sparkline as scope-creep risk) is now resolved by explicitly deferring both
to v2.1 — v2.0 ships a fully-crafted list-only surface with no dependency on them.

## Open design questions — RESOLVED (decisions locked 2026-07-25)

1. **Detail pane priority — RESOLVED: DEFERRED to v2.1.** v2.0 is list + stat chips
   only, across all breakpoints. Wide detail pane + bytes/s sparkline (and the
   `Sparkline` component + its stat-history stream) are v2.1 enhancements; no v2.0
   section depends on them (§3, §4.3, §11, §12, §15 updated).
2. **Groups depth — RESOLVED: single exclusive `groupId` + additive `tags`, flat
   (not nested).** Collapsible-group UI + tag filter retained as specced (§4.2).
3. **Accent / AA — RESOLVED: APPROVED.** Brand `#288DCC` (light) / `#4AA5E0` (dark)
   everywhere; derived `--accent-solid: #147ABD` used for filled-button backgrounds
   with white text to meet WCAG AA (4.62:1). Locked in `design-tokens.md §3`.
4. **Window chrome — RESOLVED: platform-split.** macOS = custom transparent
   titlebar with visible traffic lights + `data-tauri-drag-region` (v1 continuity).
   Windows/Linux = native OS decorations (title bar + min/max/close) owning
   drag/snap/maximize; no custom controls or drag region there (§3 rewritten).
5. **Filter + ⌘K — RESOLVED: KEEP BOTH.** Lightweight per-screen filter input
   (Connections/Activity) *and* the global command palette (⌘/Ctrl+K). They serve
   different jobs — in-place narrowing vs global launch.

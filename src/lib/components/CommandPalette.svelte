<!--
  CommandPalette — ⌘K / Ctrl+K fuzzy launcher over tunnels + actions (spec 05
  §10). Keyboard-first: ↑↓ navigate, ↵ run primary, ⌘↵ secondary (edit), → open
  a tunnel's action sub-menu, ← / esc back out or close. Mouse hover shares the
  single active-index model. All effects go through lib/ipc.ts / the command bus
  — the palette owns no truth.
-->
<script lang="ts">
  import { get } from "svelte/store";
  import { writeText } from "@tauri-apps/plugin-clipboard-manager";
  import type { ForwardConfig, ForwardStatus } from "../types";
  import type { IconName } from "./ui/icons";
  import { forwards, statusById } from "../stores/forwards";
  import { groups } from "../stores/groups";
  import { settings } from "../stores/settings";
  import {
    paletteQuery,
    paletteRecents,
    closePalette,
    recordPaletteUse,
  } from "../stores/palette";
  import { requestAddForm, requestEditForm, requestDelete } from "../stores/commands";
  import { activeView } from "../ui/view";
  import { pushToast } from "../ui/toast";
  import {
    connectForward,
    disconnectForward,
    retryForward,
    duplicateForward,
    copySshCommand,
    startAll,
    stopAll,
    startGroup,
    stopGroup,
    checkUpdate,
    updateSettings,
  } from "../ipc";
  import { fuzzyRank } from "../fuzzy";
  import Icon from "./ui/Icon.svelte";
  import StatusDot from "./ui/StatusDot.svelte";

  const VERSION = "2.0.0";

  interface PaletteItem {
    id: string;
    title: string;
    subtitle?: string;
    icon?: IconName;
    status?: ForwardStatus;
    hint?: string;
    haystack: string;
    forward?: ForwardConfig;
    // Effect fns may return a toast id / promise; the caller discards it.
    run: () => unknown;
    runSecondary?: () => unknown;
  }

  let inputEl = $state<HTMLInputElement | undefined>();
  let activeIndex = $state(0);
  // When set, the list shows that tunnel's full action sub-menu (spec §10 →).
  let submenuFor = $state<ForwardConfig | null>(null);

  const q = $derived($paletteQuery);

  // --- Action wrappers (all through ipc / command bus) ---
  function close(): void {
    closePalette();
  }

  async function toast<T>(p: Promise<T>, ok: string, fail: string): Promise<void> {
    try {
      await p;
      pushToast(ok, { tone: "success" });
    } catch (err) {
      pushToast(`${fail}: ${String(err)}`, { tone: "error" });
    }
  }

  async function copyCmd(f: ForwardConfig): Promise<void> {
    try {
      await writeText(await copySshCommand(f.id));
      pushToast("SSH command copied", { tone: "success" });
    } catch (err) {
      pushToast(`Copy failed: ${String(err)}`, { tone: "error" });
    }
  }

  function toggleTheme(): void {
    const s = get(settings);
    if (!s) return;
    const dark =
      s.themeMode === "dark" ||
      (s.themeMode === "system" &&
        window.matchMedia("(prefers-color-scheme: dark)").matches);
    void toast(
      updateSettings({ ...s, themeMode: dark ? "light" : "dark" }),
      dark ? "Switched to light theme" : "Switched to dark theme",
      "Theme change failed",
    );
  }

  function isOn(status: ForwardStatus): boolean {
    return status === "connected" || status === "connecting";
  }

  // --- Tunnel items (primary = context-aware connect/disconnect) ---
  const tunnelItems = $derived<PaletteItem[]>(
    $forwards.map((f) => {
      const status = $statusById[f.id] ?? "disconnected";
      const on = isOn(status);
      const route = `${f.localBindAddress}:${f.localPort} → ${f.remoteHost}:${f.remotePort}`;
      return {
        id: `tunnel:${f.id}`,
        title: f.name,
        subtitle: route,
        status,
        hint: on ? "Disconnect" : "Connect",
        haystack: `${f.name} ${f.sshHost} ${route} ${f.tags.join(" ")}`,
        forward: f,
        run: () =>
          on
            ? void toast(disconnectForward(f.id), `Disconnecting ${f.name}`, "Disconnect failed")
            : void toast(connectForward(f.id), `Connecting ${f.name}`, "Connect failed"),
        runSecondary: () => requestEditForm(f),
      };
    }),
  );

  // --- Static + per-group action items ---
  const actionItems = $derived<PaletteItem[]>([
    {
      id: "action:add",
      title: "Add tunnel",
      icon: "plus",
      hint: "⌘N",
      haystack: "add tunnel new create forward",
      run: requestAddForm,
    },
    {
      id: "action:start-all",
      title: "Start all tunnels",
      icon: "play",
      hint: "⌘⇧⏎",
      haystack: "start all connect every tunnel",
      run: () => void toast(startAll(), "Starting all tunnels", "Start all failed"),
    },
    {
      id: "action:stop-all",
      title: "Stop all tunnels",
      icon: "power",
      hint: "⌘⇧⌫",
      haystack: "stop all disconnect every tunnel",
      run: () => void toast(stopAll(), "Stopping all tunnels", "Stop all failed"),
    },
    ...$groups.flatMap((g): PaletteItem[] => {
      const ids = new Set($forwards.filter((f) => f.groupId === g.id).map((f) => f.id));
      if (ids.size === 0) return [];
      return [
        {
          id: `action:start-group:${g.id}`,
          title: `Start all in ${g.name}`,
          icon: "play",
          haystack: `start group ${g.name}`,
          run: () =>
            void toast(startGroup(g.id), `Starting ${g.name}`, "Start group failed"),
        },
        {
          id: `action:stop-group:${g.id}`,
          title: `Stop all in ${g.name}`,
          icon: "power",
          haystack: `stop group ${g.name}`,
          run: () =>
            void toast(stopGroup(g.id), `Stopping ${g.name}`, "Stop group failed"),
        },
      ];
    }),
    {
      id: "action:go-connections",
      title: "Go to Connections",
      icon: "plug",
      hint: "⌘1",
      haystack: "go connections tunnels list",
      run: () => activeView.set("connections"),
    },
    {
      id: "action:go-activity",
      title: "Go to Activity",
      icon: "activity",
      hint: "⌘2",
      haystack: "go activity logs",
      run: () => activeView.set("activity"),
    },
    {
      id: "action:go-settings",
      title: "Open Settings",
      icon: "settings",
      hint: "⌘,",
      haystack: "open settings preferences",
      run: () => activeView.set("settings"),
    },
    {
      id: "action:toggle-theme",
      title: "Toggle theme",
      icon: "moon",
      haystack: "toggle theme dark light appearance",
      run: toggleTheme,
    },
    {
      id: "action:check-update",
      title: "Check for updates",
      icon: "refresh-cw",
      haystack: "check updates version",
      run: () =>
        void checkUpdate()
          .then((s) =>
            pushToast(
              s.available ? `Version ${s.version} available` : "You're on the latest version",
              { tone: "info" },
            ),
          )
          .catch(() => pushToast("Update checks arrive in a later build", { tone: "info" })),
    },
    {
      id: "action:about",
      title: `About Tunnel Pilot v${VERSION}`,
      icon: "info",
      haystack: "about version",
      run: () => pushToast(`Tunnel Pilot v${VERSION}`, { tone: "info" }),
    },
  ]);

  // --- Sub-menu items for a single tunnel (spec §10 →) ---
  function submenuItems(f: ForwardConfig): PaletteItem[] {
    const status = $statusById[f.id] ?? "disconnected";
    const on = isOn(status);
    const items: PaletteItem[] = [];
    if (on) {
      items.push({
        id: "sub:disconnect",
        title: "Disconnect",
        icon: "power",
        haystack: "disconnect",
        run: () => void toast(disconnectForward(f.id), `Disconnecting ${f.name}`, "Disconnect failed"),
      });
    } else {
      items.push({
        id: "sub:connect",
        title: "Connect",
        icon: "play",
        haystack: "connect",
        run: () => void toast(connectForward(f.id), `Connecting ${f.name}`, "Connect failed"),
      });
    }
    if (status === "error") {
      items.push({
        id: "sub:retry",
        title: "Retry",
        icon: "rotate-cw",
        haystack: "retry",
        run: () => void toast(retryForward(f.id), `Retrying ${f.name}`, "Retry failed"),
      });
    }
    items.push(
      {
        id: "sub:edit",
        title: "Edit",
        icon: "pencil",
        haystack: "edit",
        run: () => requestEditForm(f),
      },
      {
        id: "sub:duplicate",
        title: "Duplicate",
        icon: "files",
        haystack: "duplicate",
        run: () => void toast(duplicateForward(f.id), "Tunnel duplicated", "Duplicate failed"),
      },
      {
        id: "sub:copy",
        title: "Copy SSH command",
        icon: "terminal",
        haystack: "copy ssh command",
        run: () => void copyCmd(f),
      },
      {
        id: "sub:delete",
        title: "Delete",
        icon: "trash",
        haystack: "delete remove",
        run: () => requestDelete(f),
      },
    );
    return items;
  }

  // Recents float to the front on an empty query (cheap relevance signal).
  function applyRecents(items: PaletteItem[]): PaletteItem[] {
    if (q.trim() !== "") return items;
    const order = $paletteRecents;
    if (order.length === 0) return items;
    return [...items].sort((a, b) => {
      const ia = order.indexOf(a.id);
      const ib = order.indexOf(b.id);
      return (ia === -1 ? Infinity : ia) - (ib === -1 ? Infinity : ib);
    });
  }

  const rankedTunnels = $derived(
    applyRecents(fuzzyRank(q, tunnelItems, (i) => i.haystack).map((r) => r.item)),
  );
  const rankedActions = $derived(
    applyRecents(fuzzyRank(q, actionItems, (i) => i.haystack).map((r) => r.item)),
  );
  const rankedSub = $derived(
    submenuFor
      ? fuzzyRank(q, submenuItems(submenuFor), (i) => i.haystack).map((r) => r.item)
      : [],
  );

  // Flattened, ordered list the keyboard drives (sections are a render concern).
  const flat = $derived<PaletteItem[]>(
    submenuFor ? rankedSub : [...rankedTunnels, ...rankedActions],
  );

  // Section render model: interleave headers with their items (index-aligned to `flat`).
  interface Row {
    header?: string;
    item?: PaletteItem;
    index?: number;
  }
  const rows = $derived<Row[]>(
    (() => {
      const out: Row[] = [];
      if (submenuFor) {
        out.push({ header: submenuFor.name.toUpperCase() });
        rankedSub.forEach((item, i) => out.push({ item, index: i }));
        return out;
      }
      let i = 0;
      if (rankedTunnels.length > 0) {
        out.push({ header: "Tunnels" });
        for (const item of rankedTunnels) out.push({ item, index: i++ });
      }
      if (rankedActions.length > 0) {
        out.push({ header: "Actions" });
        for (const item of rankedActions) out.push({ item, index: i++ });
      }
      return out;
    })(),
  );

  // Keep the active index in range as the list changes.
  $effect(() => {
    const n = flat.length;
    if (activeIndex >= n) activeIndex = Math.max(0, n - 1);
    if (activeIndex < 0) activeIndex = 0;
  });

  // Reset selection to the top whenever the query changes or we enter/leave a submenu.
  $effect(() => {
    void q;
    void submenuFor;
    activeIndex = 0;
  });

  $effect(() => {
    inputEl?.focus();
  });

  function runItem(item: PaletteItem): void {
    recordPaletteUse(submenuFor ? `tunnel:${submenuFor.id}` : item.id);
    void item.run();
    close();
  }

  function openSubmenu(item: PaletteItem): void {
    if (item.forward) {
      submenuFor = item.forward;
      paletteQuery.set("");
    }
  }

  function backOut(): void {
    if (submenuFor) {
      submenuFor = null;
      paletteQuery.set("");
    } else {
      close();
    }
  }

  function onKeydown(e: KeyboardEvent): void {
    const item = flat[activeIndex];
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        activeIndex = flat.length === 0 ? 0 : (activeIndex + 1) % flat.length;
        break;
      case "ArrowUp":
        e.preventDefault();
        activeIndex = flat.length === 0 ? 0 : (activeIndex - 1 + flat.length) % flat.length;
        break;
      case "Enter":
        e.preventDefault();
        if (!item) return;
        if ((e.metaKey || e.ctrlKey) && item.runSecondary) {
          recordPaletteUse(item.id);
          void item.runSecondary();
          close();
        } else {
          runItem(item);
        }
        break;
      case "ArrowRight":
        if (item?.forward && !submenuFor) {
          e.preventDefault();
          openSubmenu(item);
        }
        break;
      case "ArrowLeft":
        if (submenuFor) {
          e.preventDefault();
          backOut();
        }
        break;
      case "Escape":
        e.preventDefault();
        backOut();
        break;
    }
  }

  const activeId = $derived(`palette-opt-${activeIndex}`);
</script>

<div
  class="scrim"
  role="presentation"
  onclick={(e) => {
    if (e.target === e.currentTarget) close();
  }}
>
  <div class="palette" role="dialog" aria-modal="true" aria-label="Command palette">
    <div class="search">
      <Icon name="search" size={16} />
      <input
        bind:this={inputEl}
        class="input"
        type="text"
        role="combobox"
        aria-expanded="true"
        aria-controls="palette-listbox"
        aria-activedescendant={flat.length > 0 ? activeId : undefined}
        aria-autocomplete="list"
        placeholder={submenuFor ? `Actions for ${submenuFor.name}…` : "Search tunnels and actions…"}
        autocomplete="off"
        spellcheck="false"
        bind:value={$paletteQuery}
        onkeydown={onKeydown}
      />
      {#if submenuFor}
        <button type="button" class="back" onclick={backOut} title="Back (Esc)">
          <Icon name="corner-down-left" size={13} /> Back
        </button>
      {/if}
    </div>

    <ul id="palette-listbox" class="list" role="listbox" aria-label="Results">
      {#each rows as row (row.header ? `h:${row.header}` : row.item?.id)}
        {#if row.header}
          <li class="section" role="presentation">{row.header}</li>
        {:else if row.item}
          {@const item = row.item}
          {@const idx = row.index ?? 0}
          <li role="presentation">
            <button
              type="button"
              id="palette-opt-{idx}"
              class="opt"
              class:active={idx === activeIndex}
              role="option"
              aria-selected={idx === activeIndex}
              onmousemove={() => (activeIndex = idx)}
              onclick={() => runItem(item)}
            >
              <span class="lead" aria-hidden="true">
                {#if item.status}
                  <StatusDot status={item.status} />
                {:else if item.icon}
                  <Icon name={item.icon} size={16} />
                {/if}
              </span>
              <span class="text">
                <span class="opt-title">{item.title}</span>
                {#if item.subtitle}
                  <span class="opt-sub mono">{item.subtitle}</span>
                {/if}
              </span>
              {#if item.hint}
                <span class="opt-hint" class:kbd={item.hint.length <= 4}>{item.hint}</span>
              {/if}
              {#if item.forward && !submenuFor}
                <span class="chev" aria-hidden="true"><Icon name="chevron-right" size={14} /></span>
              {/if}
            </button>
          </li>
        {/if}
      {/each}

      {#if flat.length === 0}
        <li class="empty" role="presentation">
          <p class="empty-title">No results for “{q.trim()}”</p>
          <p class="empty-hint">Press ⌘N to add a tunnel.</p>
        </li>
      {/if}
    </ul>

    <div class="footer" aria-hidden="true">
      <span><kbd>↑</kbd><kbd>↓</kbd> navigate</span>
      <span><kbd>↵</kbd> run</span>
      <span><kbd>⌘↵</kbd> edit</span>
      <span><kbd>→</kbd> actions</span>
      <span><kbd>esc</kbd> close</span>
    </div>
  </div>

  <div class="sr-only" role="status" aria-live="polite">
    {flat.length}
    {flat.length === 1 ? "result" : "results"}
  </div>
</div>

<style>
  .scrim {
    position: fixed;
    inset: 0;
    z-index: var(--z-palette);
    background: var(--scrim);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding: 15vh var(--sp-6) var(--sp-6);
    animation: scrim-in var(--dur-fast) var(--ease-standard);
  }
  .palette {
    width: 100%;
    max-width: 560px;
    max-height: 60vh;
    display: flex;
    flex-direction: column;
    background: var(--surface-overlay);
    border: var(--border-w) solid var(--border);
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-3);
    overflow: hidden;
    animation: palette-in var(--dur-slow) var(--ease-decel);
  }

  .search {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    padding: var(--sp-4) var(--sp-5);
    color: var(--text-3);
    border-bottom: var(--border-w) solid var(--divider);
  }
  .input {
    flex: 1;
    min-width: 0;
    border: none;
    background: transparent;
    color: var(--text);
    font-size: var(--fs-body-lg);
    line-height: var(--lh-body-lg);
  }
  .input:focus {
    outline: none;
  }
  .input::placeholder {
    color: var(--text-3);
  }
  .back {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    flex: none;
    padding: var(--sp-1) var(--sp-2);
    border: var(--border-w) solid var(--border);
    border-radius: var(--radius-xs);
    background: var(--surface-2);
    color: var(--text-2);
    font-size: var(--fs-body-sm);
    cursor: pointer;
  }
  .back:hover {
    color: var(--text);
  }

  .list {
    flex: 1;
    margin: 0;
    padding: var(--sp-2);
    list-style: none;
    overflow-y: auto;
  }
  .section {
    padding: var(--sp-3) var(--sp-3) var(--sp-1);
    font-size: var(--fs-label);
    line-height: var(--lh-label);
    font-weight: var(--fw-label);
    letter-spacing: var(--tracking-label);
    text-transform: uppercase;
    color: var(--text-3);
  }
  .opt {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    width: 100%;
    padding: var(--sp-3);
    border: none;
    border-radius: var(--radius-sm);
    background: transparent;
    color: var(--text);
    text-align: left;
    cursor: pointer;
  }
  .opt.active {
    background: var(--accent-subtle-2);
  }
  .lead {
    display: grid;
    place-items: center;
    width: 16px;
    flex: none;
    color: var(--text-2);
  }
  .opt.active .lead {
    color: var(--accent-text);
  }
  .text {
    display: flex;
    align-items: baseline;
    gap: var(--sp-3);
    min-width: 0;
    flex: 1;
  }
  .opt-title {
    flex: none;
    max-width: 55%;
    font-size: var(--fs-body);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .opt-sub {
    min-width: 0;
    font-size: var(--fs-mono-sm);
    color: var(--text-3);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .opt-hint {
    flex: none;
    font-size: var(--fs-body-sm);
    color: var(--text-2);
  }
  .opt-hint.kbd {
    font-family: var(--font-mono);
    padding: var(--sp-1) var(--sp-2);
    border-radius: var(--radius-xs);
    background: var(--surface-2);
    color: var(--text-3);
    font-size: var(--fs-mono-sm);
  }
  .chev {
    flex: none;
    color: var(--text-3);
  }

  .empty {
    padding: var(--sp-7) var(--sp-4);
    text-align: center;
  }
  .empty-title {
    margin: 0;
    font-size: var(--fs-body);
    color: var(--text-2);
  }
  .empty-hint {
    margin: var(--sp-2) 0 0;
    font-size: var(--fs-body-sm);
    color: var(--text-3);
  }

  .footer {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-4);
    padding: var(--sp-2) var(--sp-5);
    border-top: var(--border-w) solid var(--divider);
    font-size: var(--fs-body-sm);
    color: var(--text-3);
  }
  .footer kbd {
    font-family: var(--font-mono);
    font-size: var(--fs-mono-sm);
    color: var(--text-2);
    margin-right: 2px;
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }

  @keyframes scrim-in {
    from {
      opacity: 0;
    }
    to {
      opacity: 1;
    }
  }
  @keyframes palette-in {
    from {
      opacity: 0;
      transform: translateY(-8px) scale(0.98);
    }
    to {
      opacity: 1;
      transform: none;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .scrim,
    .palette {
      animation: none;
    }
  }
</style>

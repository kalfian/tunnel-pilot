<script lang="ts">
  import { fade } from "svelte/transition";
  import { connectedCount } from "./lib/stores/forwards";
  import { keychainUnavailable } from "./lib/stores/settings";
  import { activeView, type ViewId } from "./lib/ui/view";
  import { isMacOS } from "./lib/ui/platform";
  import { paletteOpen, togglePalette } from "./lib/stores/palette";
  import { requestAddForm } from "./lib/stores/commands";
  import SidebarItem from "./lib/components/ui/SidebarItem.svelte";
  import Icon from "./lib/components/ui/Icon.svelte";
  import ToastHost from "./lib/components/ui/ToastHost.svelte";
  import CommandPalette from "./lib/components/CommandPalette.svelte";
  import ConnectionsView from "./lib/routes/ConnectionsView.svelte";
  import LogsView from "./lib/routes/LogsView.svelte";
  import SettingsView from "./lib/routes/SettingsView.svelte";

  const mac = isMacOS();

  // Compact rail (icon-only) on narrow windows (spec §3 Compact < 640).
  let compact = $state(false);
  let reducedMotion = $state(false);
  $effect(() => {
    const narrow = window.matchMedia("(max-width: 640px)");
    const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
    const sync = (): void => {
      compact = narrow.matches;
      reducedMotion = motion.matches;
    };
    sync();
    narrow.addEventListener("change", sync);
    motion.addEventListener("change", sync);
    return () => {
      narrow.removeEventListener("change", sync);
      motion.removeEventListener("change", sync);
    };
  });

  const NAV: {
    id: ViewId;
    icon: "plug" | "activity" | "settings";
    label: string;
  }[] = [
    { id: "connections", icon: "plug", label: "Connections" },
    { id: "activity", icon: "activity", label: "Activity" },
    { id: "settings", icon: "settings", label: "Settings" },
  ];

  function onKeydown(e: KeyboardEvent): void {
    if (!(e.metaKey || e.ctrlKey)) return;
    // ⌘K / Ctrl+K toggles the palette from anywhere (even over a dialog).
    if (e.key === "k") {
      e.preventDefault();
      togglePalette();
      return;
    }
    // While the palette owns focus, let it handle its own keys (it manages
    // ⌘N/⌘,/nav internally via its action list).
    if ($paletteOpen) return;
    if (e.key === "n") {
      e.preventDefault();
      requestAddForm();
    } else if (e.key === "1") {
      e.preventDefault();
      activeView.set("connections");
    } else if (e.key === "2") {
      e.preventDefault();
      activeView.set("activity");
    } else if (e.key === "3" || e.key === ",") {
      e.preventDefault();
      activeView.set("settings");
    }
  }

  const fadeMs = $derived(reducedMotion ? 0 : 150);
</script>

<svelte:window onkeydown={onKeydown} />

<div class="app" class:mac>
  {#if mac}
    <!-- macOS: custom transparent titlebar = drag region; native traffic lights
         sit at the left, so pad the title clear of them (spec §3). -->
    <div class="titlebar" data-tauri-drag-region>
      <span class="app-name" data-tauri-drag-region>Tunnel Pilot</span>
    </div>
  {/if}

  <div class="main">
    <nav class="rail" class:compact aria-label="Primary">
      <div class="rail-items">
        {#each NAV as item (item.id)}
          <SidebarItem
            icon={item.icon}
            label={item.label}
            active={$activeView === item.id}
            badge={item.id === "connections" ? $connectedCount : undefined}
            {compact}
            onclick={() => activeView.set(item.id)}
          />
        {/each}
      </div>

      <!-- Persistent ⌘K affordance (spec §2: always visible in the rail). -->
      <button
        type="button"
        class="palette-cue"
        class:compact
        aria-label="Open command palette"
        title="Command palette (⌘K)"
        onclick={togglePalette}
      >
        <Icon name="search" size={15} />
        {#if !compact}
          <span class="cue-label">Search</span>
          <span class="cue-kbd mono">⌘K</span>
        {/if}
      </button>
    </nav>

    <main class="content">
      {#if $keychainUnavailable}
        <div class="keychain-warn" role="alert">
          <Icon name="alert-triangle" size={15} />
          <span>
            OS keychain unavailable — SSH passwords are stored in a local
            plaintext fallback file.
          </span>
        </div>
      {/if}

      <div class="view-host">
        {#key $activeView}
          <div class="view-wrap" in:fade={{ duration: fadeMs }}>
            {#if $activeView === "connections"}
              <ConnectionsView />
            {:else if $activeView === "activity"}
              <LogsView />
            {:else}
              <SettingsView />
            {/if}
          </div>
        {/key}
      </div>
    </main>
  </div>

  {#if $paletteOpen}
    <CommandPalette />
  {/if}

  <ToastHost />
</div>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg);
    color: var(--text);
  }
  .titlebar {
    flex: none;
    height: var(--titlebar-h);
    display: flex;
    align-items: center;
    /* clear the native traffic lights on the left (spec §3 ~76px) */
    padding-left: 76px;
    border-bottom: var(--border-w) solid var(--divider);
  }
  .app-name {
    font-size: var(--fs-body-sm);
    font-weight: 600;
    color: var(--text-2);
    pointer-events: none;
  }

  .main {
    flex: 1;
    display: grid;
    grid-template-columns: 176px 1fr;
    min-height: 0;
  }
  .main:has(.rail.compact) {
    grid-template-columns: 52px 1fr;
  }

  .rail {
    display: flex;
    flex-direction: column;
    padding: var(--sp-4) var(--sp-3);
    border-right: var(--border-w) solid var(--divider);
    background: var(--bg);
    overflow: hidden;
  }
  .rail.compact {
    padding: var(--sp-4) var(--sp-2);
    align-items: center;
  }
  .rail-items {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    width: 100%;
  }

  .palette-cue {
    margin-top: auto;
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    width: 100%;
    padding: var(--sp-2) var(--sp-3);
    border: var(--border-w) solid var(--border);
    border-radius: var(--radius-sm);
    background: var(--surface-2);
    color: var(--text-2);
    cursor: pointer;
    transition:
      background-color var(--dur-fast) var(--ease-standard),
      border-color var(--dur-fast) var(--ease-standard);
  }
  .palette-cue.compact {
    width: var(--hit-min);
    height: var(--hit-min);
    justify-content: center;
    padding: 0;
  }
  .palette-cue:hover {
    background: var(--hover);
    border-color: var(--border-strong);
    color: var(--text);
  }
  .palette-cue:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: 2px;
  }
  .cue-label {
    flex: 1;
    text-align: left;
    font-size: var(--fs-body-sm);
  }
  .cue-kbd {
    flex: none;
    font-size: var(--fs-mono-sm);
    color: var(--text-3);
  }

  .content {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
    /* Establish a container so screens reflow to the CONTENT-area width
       (excludes the rail), per spec §3 breakpoints. */
    container: content / inline-size;
  }
  .keychain-warn {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    padding: var(--sp-2) var(--sp-6);
    background: var(--status-pending-bg);
    color: var(--status-pending-fg);
    font-size: var(--fs-body-sm);
    border-bottom: var(--border-w) solid var(--divider);
  }
  .view-host {
    position: relative;
    flex: 1;
    min-height: 0;
  }
  .view-wrap {
    position: absolute;
    inset: 0;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
</style>

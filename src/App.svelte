<script lang="ts">
  // M0 placeholder shell. The real tab nav (Connections / Logs / Settings) +
  // command-palette host lands in M4/M5 per spec 02 §4 and 05-UI-UX-SPEC.md.
  const version = "2.0.0";
</script>

<main class="shell" data-tauri-drag-region>
  <h1>Tunnel Pilot</h1>
  <p>v{version} — Tauri v2 rewrite scaffold (M0)</p>
</main>

<style>
  .shell {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    height: 100%;
    gap: 0.5rem;
    text-align: center;
  }

  h1 {
    margin: 0;
    font-size: 1.5rem;
    font-weight: 600;
  }

  p {
    margin: 0;
    opacity: 0.6;
    font-size: 0.875rem;
  }
</style>

<script lang="ts">
  interface Props {
    variant?: "card" | "row" | "line";
  }
  const { variant = "line" }: Props = $props();
</script>

<div class="sk {variant}" aria-hidden="true"></div>

<style>
  .sk {
    border-radius: var(--radius-md);
    background: linear-gradient(
      90deg,
      var(--surface-2) 0%,
      var(--surface-3) 50%,
      var(--surface-2) 100%
    );
    background-size: 200% 100%;
    animation: sk-shimmer var(--dur-slow) linear infinite alternate;
  }
  .card {
    height: var(--row-h);
  }
  .row {
    height: var(--log-row-h);
    border-radius: var(--radius-xs);
  }
  .line {
    height: 12px;
    border-radius: var(--radius-xs);
  }
  @keyframes sk-shimmer {
    to {
      background-position: -100% 0;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .sk {
      animation: none;
      background: var(--surface-2);
    }
  }
</style>

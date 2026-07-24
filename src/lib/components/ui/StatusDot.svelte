<script lang="ts">
  import type { ForwardStatus } from "../../types";

  interface Props {
    status: ForwardStatus;
    size?: number;
  }

  const { status, size = 8 }: Props = $props();

  const LABEL: Record<ForwardStatus, string> = {
    disconnected: "Disconnected",
    connecting: "Connecting",
    connected: "Connected",
    disconnecting: "Disconnecting",
    error: "Error",
  };

  const pending = $derived(
    status === "connecting" || status === "disconnecting",
  );
</script>

<span
  class="dot {status}"
  class:pulse={pending}
  style="--dot-size: {size}px"
  role="img"
  aria-label={LABEL[status]}
></span>

<style>
  .dot {
    display: inline-block;
    flex: none;
    width: var(--dot-size);
    height: var(--dot-size);
    border-radius: var(--radius-full);
    transition: background-color var(--dur-instant) var(--ease-standard);
  }
  .disconnected {
    background: var(--status-idle);
  }
  .connected {
    background: var(--status-connected);
  }
  .error {
    background: var(--status-error);
  }
  .connecting,
  .disconnecting {
    background: var(--status-pending);
  }
  .pulse {
    animation: dot-pulse 1s ease-in-out infinite;
  }
  @keyframes dot-pulse {
    0%,
    100% {
      opacity: 1;
    }
    50% {
      opacity: 0.4;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .pulse {
      animation: none;
      opacity: 1;
    }
  }
</style>

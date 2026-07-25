<script lang="ts">
  import { ICONS, type IconName } from "./icons";

  interface Props {
    name: IconName;
    /** px; defaults to 16 (matches --fs-body line-box). */
    size?: number;
    /** stroke width; Lucide default 2, slightly thinner reads better small. */
    stroke?: number;
    /** decorative by default; pass a label to expose it to AT. */
    label?: string;
  }

  const { name, size = 16, stroke = 1.75, label }: Props = $props();
</script>

<svg
  class="icon"
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width={stroke}
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden={label ? undefined : "true"}
  role={label ? "img" : undefined}
  aria-label={label}
>
  <!-- Static, internal icon geometry (see icons.ts) — never user input. -->
  <!-- eslint-disable-next-line svelte/no-at-html-tags -->
  {@html ICONS[name]}
</svg>

<style>
  .icon {
    display: block;
    flex: none;
  }
</style>

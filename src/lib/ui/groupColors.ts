/**
 * Group color palette (spec 05 §4.2: "a small palette of tokened accent/status
 * colors"). A group stores a stable string KEY (not a raw hex), so the swatch
 * re-derives to the theme-aware CSS custom property at render — a hex would be
 * frozen to one theme. `null` = no color → falls back to the brand accent.
 *
 * Restraint by design: five choices, every one already a semantic token from
 * `design-tokens.md §3` (no invented hues). Shared by the group form, the group
 * header rail, and the tray popover section labels so grouping reads the same
 * everywhere.
 */

export interface GroupColorOption {
  /** Persisted value on `TunnelGroup.color`. */
  key: string;
  label: string;
  /** Semantic token the swatch/rail resolves to. */
  cssVar: string;
}

export const GROUP_COLORS: readonly GroupColorOption[] = [
  { key: "blue", label: "Blue", cssVar: "--accent" },
  { key: "green", label: "Green", cssVar: "--status-connected" },
  { key: "amber", label: "Amber", cssVar: "--status-pending" },
  { key: "red", label: "Red", cssVar: "--status-error" },
  { key: "gray", label: "Gray", cssVar: "--status-idle" },
] as const;

/**
 * Resolve a persisted group color to a `var(--…)` reference. Unknown/null keys
 * fall back to the brand accent so a group always has a coherent marker.
 */
export function groupColorVar(color: string | null): string {
  const match = GROUP_COLORS.find((c) => c.key === color);
  return `var(${match ? match.cssVar : "--accent"})`;
}

import { settings, type ColorLabel } from "./stores/settings.svelte";

export function formatColorLabel(label: string): string {
  const name = settings.colorLabelNames[label as ColorLabel];
  return name ? `${name} (${label})` : label;
}

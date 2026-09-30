import { keymap } from "./dispatcher.svelte";
import { formatKey, type CommandId } from "./keymap";

export function shortcutHint(label: string, command: CommandId): string {
  const keys = [...keymap.bindings.entries()]
    .filter(([, id]) => id === command)
    .map(([key]) => formatKey(key));
  return keys.length ? `${label} (${keys.join(" / ")})` : label;
}

import { SvelteSet } from "svelte/reactivity";
import { videoUrl, type ItemLite } from "../api";

// App session only. File IDs can repeat across projects and file versions.
export const unplayableVideos = new SvelteSet<string>();

export function playbackFailureKey(item: ItemLite): string {
  return `${videoUrl(item)}&b=${encodeURIComponent(item.sourceVersion ?? "")}`;
}

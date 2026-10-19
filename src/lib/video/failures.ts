import { SvelteMap } from "svelte/reactivity";
import { videoUrl, type ItemLite } from "../api";
import type { PlaybackReason } from "./codecs";

// App session only. File IDs can repeat across projects and file versions.
export const unplayableVideos = new SvelteMap<string, PlaybackReason | null>();

export function playbackFailureKey(item: ItemLite): string {
  return `${videoUrl(item)}&b=${encodeURIComponent(item.sourceVersion ?? "")}`;
}

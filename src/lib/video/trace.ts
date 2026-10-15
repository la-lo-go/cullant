/**
 * Opt-in trace of the video playback ladder, for diagnosing a clip that stops
 * on a device: every range read, the remux decisions and appends, and the
 * element's own events, as `[video]` console lines with a millisecond clock.
 *
 * Enable it in the WebView console with
 *   localStorage.setItem("cullant.trace.video", "1")
 * and read the lines over the remote inspector. Off, it costs one cached check.
 */

let enabled: boolean | null = null;

function isEnabled(): boolean {
  if (enabled === null) {
    try {
      enabled = localStorage.getItem("cullant.trace.video") === "1";
    } catch {
      enabled = false;
    }
  }
  return enabled;
}

export function videoTrace(event: string, data: Record<string, unknown> = {}) {
  if (!isEnabled()) return;
  console.info("[video]", event, JSON.stringify({ t: Math.round(performance.now()), ...data }));
}

/** The element's buffered ranges, as `[start, end]` pairs in seconds. */
export function bufferedRanges(element: HTMLMediaElement | SourceBuffer | null | undefined) {
  if (!element) return [];
  const ranges: [number, number][] = [];
  try {
    for (let i = 0; i < element.buffered.length; i++) {
      ranges.push([
        Math.round(element.buffered.start(i) * 10) / 10,
        Math.round(element.buffered.end(i) * 10) / 10,
      ]);
    }
  } catch {
    // A SourceBuffer removed from its MediaSource throws on access.
  }
  return ranges;
}

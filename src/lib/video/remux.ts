/**
 * In-app playback fallback for clips the WebView refuses to open.
 *
 * The `<video>` element rejects a file for two very different reasons: it
 * cannot decode the codec, or it will not accept the container. Only the second
 * is recoverable, and it is the common one — a device that plays H.264 happily
 * still turns down plenty of containers that carry it.
 *
 * So when native playback fails, demux the file in JavaScript and rewrite it
 * into fragmented MP4, then feed that to the same `<video>` element through
 * Media Source Extensions. The encoded packets are copied, never re-encoded, so
 * the platform still hardware-decodes and audio, seeking and battery life all
 * survive. What changes is only the wrapper the element was objecting to.
 *
 * Mediabunny is imported dynamically when a video opens, so its metadata check
 * does not add to the app's startup bundle.
 */

// Type-only, so it is erased at build time and pulls nothing into this module's
// chunk — mediabunny itself is imported dynamically below.
import type { StreamTargetChunk } from "mediabunny";

import { videoUrl, type ItemLite } from "../api";
import { IS_ANDROID } from "../platform";

/** Raised when the fallback cannot help, so the caller offers an external app. */
export class UnplayableError extends Error {
  constructor(message: string, readonly retryable = false) {
    super(message);
  }
}

/**
 * The protocol answers every video request with at most an 8 MB window, so a
 * wider read comes back quietly truncated. Ask in windows it will honour rather
 * than relaxing the cap — it exists so a huge clip never lands in memory whole.
 */
const WINDOW = 8 * 1024 * 1024;
const MAX_METADATA_RANGE = 64 * 1024 * 1024;

async function range(url: string, start: number, endInclusive: number): Promise<Response> {
  const requested = `bytes=${start}-${endInclusive}`;
  // WebView seeks twice when Range accompanies an intercepted partial body.
  const request = IS_ANDROID
    ? fetch(`${url}&range=${encodeURIComponent(requested)}`)
    : fetch(url, { headers: { Range: requested } });
  const res = await request.catch(() => {
    throw new UnplayableError("video storage could not be read", true);
  });
  if (!res.ok) throw new UnplayableError(`video range request failed: ${res.status}`, true);
  return res;
}

/** Total byte length, taken from the `Content-Range` of a one-byte probe. */
async function totalSize(url: string): Promise<number> {
  const res = await range(url, 0, 0);
  const total = Number(res.headers.get("Content-Range")?.split("/")[1]);
  if (!Number.isFinite(total) || total <= 0) {
    throw new UnplayableError("video response carried no usable Content-Range", true);
  }
  return total;
}

/** Read `[start, end)` from the protocol, in windows it will actually serve. */
async function readRange(url: string, start: number, end: number): Promise<Uint8Array> {
  if (!Number.isSafeInteger(start) || !Number.isSafeInteger(end) || start < 0 || end < start || end - start > MAX_METADATA_RANGE) {
    throw new UnplayableError("video metadata range exceeds the memory limit");
  }
  const out = new Uint8Array(end - start);
  let at = start;
  while (at < end) {
    const stop = Math.min(at + WINDOW, end);
    const res = await range(url, at, stop - 1);
    const chunk = new Uint8Array(await res.arrayBuffer().catch(() => {
      throw new UnplayableError("video read was interrupted", true);
    }));
    if (chunk.byteLength === 0) throw new UnplayableError("video range returned no bytes", true);
    if (chunk.byteLength > stop - at) throw new UnplayableError("video range returned too many bytes");
    out.set(chunk, at - start);
    at += chunk.byteLength;
  }
  return out;
}

export type Remux = {
  src: string;
  done: Promise<void>;
  cancel: () => void;
};

async function openInput(item: ItemLite) {
  const { Input, ALL_FORMATS, CustomSource } = await import("mediabunny");
  const url = videoUrl(item);
  return new Input({
    formats: ALL_FORMATS,
    source: new CustomSource({
      getSize: () => totalSize(url),
      read: (start, end) => readRange(url, start, end),
      prefetchProfile: "network",
    }),
  });
}

/** Inspect the codec without playing or preparing the video. */
export async function needsPlaybackFallback(item: ItemLite, canPlay: (mime: string) => boolean): Promise<boolean> {
  // Large native clips need nonzero Range reads, which Android's intercepted
  // stream cannot serve correctly. Remux uses the explicit query above.
  if (IS_ANDROID && await totalSize(videoUrl(item)) > WINDOW) return true;
  const input = await openInput(item);
  try {
    const track = await input.getPrimaryVideoTrack();
    const codec = await track?.getCodecParameterString();
    return !!codec && !canPlay(`video/mp4; codecs="${codec}"`);
  } finally {
    input.dispose();
  }
}

/**
 * Rewrite `item` into fragmented MP4 behind a `MediaSource` and hand back a URL
 * for the video element.
 *
 * Throws [[UnplayableError]] without doing any work when the result would not
 * be playable anyway. That check is the important one: mediabunny transcodes
 * whenever it cannot copy, and offers no way to forbid it, so a codec this
 * device cannot play would otherwise turn into a full re-encode on a phone —
 * far worse than the failure it was meant to fix.
 */
export async function remuxForPlayback(
  item: ItemLite,
  onProgress?: (fraction: number) => void,
): Promise<Remux> {
  if (typeof MediaSource === "undefined") {
    throw new UnplayableError("MediaSource is unavailable");
  }
  const { Output, Mp4OutputFormat, StreamTarget, Conversion } =
    await import("mediabunny");
  const input = await openInput(item);

  // Everything up to the point of no return, so a rejected clip disposes the
  // input rather than leaking it and its open requests.
  let mime: string;
  let dropAudio = false;
  try {
    const video = await input.getPrimaryVideoTrack();
    if (!video) throw new UnplayableError("no video track");
    const videoCodec = await video.getCodecParameterString();
    if (!videoCodec) throw new UnplayableError("unrecognised video codec");

    const audio = await input.getPrimaryAudioTrack();
    const audioCodec = (await audio?.getCodecParameterString()) ?? null;
    dropAudio = !!audio && !audioCodec;

    // Prefer keeping the audio, but never let it sink the clip: a container can
    // carry audio that MP4 cannot, and a silent video still lets the user judge
    // the take.
    const mimeFor = (codecs: string[]) => `video/mp4; codecs="${codecs.join(",")}"`;
    mime = audioCodec ? mimeFor([videoCodec, audioCodec]) : mimeFor([videoCodec]);
    if (!MediaSource.isTypeSupported(mime)) {
      mime = mimeFor([videoCodec]);
      dropAudio = true;
      if (!MediaSource.isTypeSupported(mime)) {
        throw new UnplayableError(`this device cannot play ${videoCodec}`);
      }
    }
  } catch (e) {
    input.dispose();
    throw e;
  }

  const mediaSource = new MediaSource();
  const src = URL.createObjectURL(mediaSource);
  let conversion: Awaited<ReturnType<typeof Conversion.init>> | null = null;
  let canceled = false;
  let finishCanceled: (() => void) | null = null;

  const done = new Promise<void>((resolve, reject) => {
    finishCanceled = resolve;
    // A MediaSource only opens once it is attached to a media element, so the
    // conversion cannot start until the caller has taken `src`.
    mediaSource.addEventListener(
      "sourceopen",
      () => {
        if (canceled) return;
        void (async () => {
          try {
            const buffer = mediaSource.addSourceBuffer(mime);
            let written = 0;

            const writable = new WritableStream<StreamTargetChunk>({
              write(chunk) {
                if (canceled) return Promise.resolve();
                // Fragmented output is documented to be written in order, and
                // appending to a SourceBuffer is the one thing that cannot cope
                // with a seek. Fail loudly rather than produce a corrupt stream.
                if (chunk.position !== written) {
                  throw new Error(
                    `remux wrote out of order at ${chunk.position}, expected ${written}`,
                  );
                }
                written += chunk.data.byteLength;
                // Resolving on updateend is also what applies backpressure:
                // appendBuffer takes one write at a time, and Output honours a
                // slow target rather than racing ahead of it.
                return new Promise<void>((ok, fail) => {
                  buffer.addEventListener("updateend", () => ok(), { once: true });
                  buffer.addEventListener("error", () => fail(new Error("append failed")), {
                    once: true,
                  });
                  buffer.appendBuffer(chunk.data);
                });
              },
            });

            const output = new Output({
              format: new Mp4OutputFormat({ fastStart: "fragmented" }),
              target: new StreamTarget(writable),
            });

            conversion = await Conversion.init({
              input,
              output,
              ...(dropAudio ? { audio: { discard: true } } : {}),
            });
            if (canceled) {
              await conversion.cancel();
              return;
            }
            if (!conversion.isValid) {
              throw new UnplayableError(
                conversion.discardedTracks.map((t) => t.reason).join(", ") || "nothing to convert",
              );
            }
            if (onProgress) {
              conversion.onProgress = onProgress;
              onProgress(0);
            }

            await conversion.execute();
            if (!canceled && mediaSource.readyState === "open") mediaSource.endOfStream();
            resolve();
          } catch (e) {
            // Cancelling is the caller moving to another clip, not a failure —
            // reporting it as one would flash "can't be played" on the way out.
            if (canceled) resolve();
            else reject(e);
          } finally {
            input.dispose();
          }
        })();
      },
      { once: true },
    );
  });

  return {
    src,
    done,
    cancel: () => {
      canceled = true;
      void conversion?.cancel();
      input.dispose();
      URL.revokeObjectURL(src);
      finishCanceled?.();
    },
  };
}

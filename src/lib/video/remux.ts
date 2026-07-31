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
 * Mediabunny is imported dynamically: it is dead weight for every clip that
 * plays normally, and this module is only reached once one has not.
 */

// Type-only, so it is erased at build time and pulls nothing into this module's
// chunk — mediabunny itself is imported dynamically below.
import type { StreamTargetChunk } from "mediabunny";

import { videoUrl, type ItemLite } from "../api";

/** Raised when the fallback cannot help, so the caller offers an external app. */
export class UnplayableError extends Error {}

/**
 * The protocol answers every video request with at most an 8 MB window, so a
 * wider read comes back quietly truncated. Ask in windows it will honour rather
 * than relaxing the cap — it exists so a huge clip never lands in memory whole.
 */
const WINDOW = 8 * 1024 * 1024;

async function range(url: string, start: number, endInclusive: number): Promise<Response> {
  const res = await fetch(url, { headers: { Range: `bytes=${start}-${endInclusive}` } });
  if (!res.ok) throw new UnplayableError(`video range request failed: ${res.status}`);
  return res;
}

/** Total byte length, taken from the `Content-Range` of a one-byte probe. */
async function totalSize(url: string): Promise<number> {
  const res = await range(url, 0, 0);
  const total = Number(res.headers.get("Content-Range")?.split("/")[1]);
  if (!Number.isFinite(total) || total <= 0) {
    throw new UnplayableError("video response carried no usable Content-Range");
  }
  return total;
}

/** Read `[start, end)` from the protocol, in windows it will actually serve. */
async function readRange(url: string, start: number, end: number): Promise<Uint8Array> {
  const out = new Uint8Array(end - start);
  let at = start;
  while (at < end) {
    const stop = Math.min(at + WINDOW, end);
    const res = await range(url, at, stop - 1);
    const chunk = new Uint8Array(await res.arrayBuffer());
    if (chunk.byteLength === 0) throw new UnplayableError("video range returned no bytes");
    out.set(chunk, at - start);
    at += chunk.byteLength;
  }
  return out;
}

/** A running remux. `src` goes on the video element; `done` settles with it. */
export type Remux = {
  src: string;
  done: Promise<void>;
  cancel: () => void;
};

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
  const { Input, ALL_FORMATS, CustomSource, Output, Mp4OutputFormat, StreamTarget, Conversion } =
    await import("mediabunny");

  const url = videoUrl(item);
  const input = new Input({
    formats: ALL_FORMATS,
    source: new CustomSource({
      getSize: () => totalSize(url),
      read: (start, end) => readRange(url, start, end),
      // Every read is an IPC round trip to the Rust protocol handler, which is
      // the high-latency profile this is for.
      prefetchProfile: "network",
    }),
  });

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

  const done = new Promise<void>((resolve, reject) => {
    // A MediaSource only opens once it is attached to a media element, so the
    // conversion cannot start until the caller has taken `src`.
    mediaSource.addEventListener(
      "sourceopen",
      () => {
        void (async () => {
          try {
            const buffer = mediaSource.addSourceBuffer(mime);
            let written = 0;

            const writable = new WritableStream<StreamTargetChunk>({
              write(chunk) {
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
            if (!conversion.isValid) {
              throw new UnplayableError(
                conversion.discardedTracks.map((t) => t.reason).join(", ") || "nothing to convert",
              );
            }
            if (onProgress) conversion.onProgress = (fraction) => onProgress(fraction);

            await conversion.execute();
            if (!canceled && mediaSource.readyState === "open") mediaSource.endOfStream();
            resolve();
          } catch (e) {
            reject(e);
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
    },
  };
}

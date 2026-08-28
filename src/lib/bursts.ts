/**
 * Grouping consecutive frames of the same burst.
 *
 * Time proposes and the perceptual hash confirms: shots close together in time
 * are candidates, and a frame that looks nothing like the one before it is
 * split off even when it falls inside the window.
 *
 * Two properties this must never violate:
 *
 * - A burst never spans two devices. Two photographers shooting the same moment
 *   produce interleaved timestamps that would otherwise merge into nonsense, so
 *   the list is partitioned by camera before any window is applied.
 * - A RAW+JPEG pair is one photo. Clustering runs over groups, not files, so a
 *   pair can never land in two different bursts.
 *
 * Note on resolution: `captureTime` is whole seconds, so a 10 fps burst reports
 * gaps of 0. That is enough to separate bursts from each other, but it means no
 * threshold can distinguish frames *inside* one.
 */

import type { ItemLite } from "./api";

/** Gap in seconds below which two shots belong to the same burst. */
export const DEFAULT_BURST_GAP_S = 2;

/**
 * Hamming distance above which two frames are different scenes, out of 64 bits.
 * Deliberately permissive: time has already narrowed the field, so the hash is
 * here to veto an obvious mismatch, not to second-guess every frame.
 */
const MAX_BURST_DISTANCE = 16;

/** Bursts smaller than this are not bursts, just photos. */
const MIN_BURST_SIZE = 2;

export type BurstMode = "fixed" | "adaptive";

/** One logical photo: a RAW+JPEG pair collapses to a single entry. */
interface Shot {
  groupId: number;
  ids: number[];
  time: number;
  camera: string;
  phash: string | null;
  /** No hash YET, as opposed to no hash ever: its thumbnail is still queued.
   *  The two look identical on the row and mean opposite things here. */
  pending: boolean;
}

function popcount32(n: number): number {
  n = n - ((n >> 1) & 0x55555555);
  n = (n & 0x33333333) + ((n >> 2) & 0x33333333);
  return (((n + (n >> 4)) & 0x0f0f0f0f) * 0x01010101) >> 24;
}

/** Bit distance between two 16-char hex hashes, done in two 32-bit halves
 *  because a 64-bit integer does not survive JavaScript's number type. */
export function hammingHex(a: string, b: string): number {
  let d = 0;
  for (let i = 0; i < 16; i += 8) {
    const x = parseInt(a.slice(i, i + 8), 16) ^ parseInt(b.slice(i, i + 8), 16);
    d += popcount32(x);
  }
  return d;
}

/** One entry per logical photo, sorted by device then time. */
function shotsOf(items: ItemLite[]): Shot[] {
  const byGroup = new Map<number, Shot>();
  for (const item of items) {
    const existing = byGroup.get(item.groupId);
    if (existing) {
      existing.ids.push(item.id);
      // The primary member speaks for the group; a JPEG sidekick scanned first
      // must not decide the pair's timestamp.
      if (item.isPrimary) {
        existing.time = item.captureTime ?? item.mtime;
        existing.camera = item.camera ?? "";
        existing.phash = item.phash;
        existing.pending = item.phash === null && !item.thumbFailed;
      }
      continue;
    }
    byGroup.set(item.groupId, {
      groupId: item.groupId,
      ids: [item.id],
      time: item.captureTime ?? item.mtime,
      camera: item.camera ?? "",
      phash: item.phash,
      pending: item.phash === null && !item.thumbFailed,
    });
  }
  return [...byGroup.values()].sort(
    (a, b) => a.camera.localeCompare(b.camera) || a.time - b.time,
  );
}

/**
 * Derive the gap from the shoot's own rhythm. Intervals within a burst and
 * intervals between bursts form two clusters; this looks for the widest ratio
 * jump between them and puts the threshold there.
 *
 * Returns null when there is no clear split — a sports shoot and a wedding do
 * not photograph alike, but neither does a folder of unrelated snapshots, and
 * guessing at one is worse than falling back to the configured gap.
 */
export function adaptiveGap(items: ItemLite[]): number | null {
  const shots = shotsOf(items);
  const gaps: number[] = [];
  for (let i = 1; i < shots.length; i++) {
    if (shots[i].camera !== shots[i - 1].camera) continue;
    gaps.push(shots[i].time - shots[i - 1].time);
  }
  if (gaps.length < 8) return null;
  gaps.sort((a, b) => a - b);

  // Ignore the extreme tails so one outlier cannot define the threshold.
  const lo = Math.floor(gaps.length * 0.05);
  const hi = Math.ceil(gaps.length * 0.95);
  let bestRatio = 1;
  let bestGap = 0;
  for (let i = lo + 1; i < hi; i++) {
    const ratio = gaps[i] / Math.max(gaps[i - 1], 1);
    if (ratio > bestRatio) {
      bestRatio = ratio;
      bestGap = gaps[i - 1];
    }
  }
  // A weak jump means the intervals are not actually bimodal.
  if (bestRatio < 3) return null;
  return Math.min(30, Math.max(1, Math.round(bestGap * 1.5) || 1));
}

export interface Bursts {
  /** file id -> burst key. A miss means the photo stands on its own. */
  byFile: Map<number, string>;
  /** burst key -> how many PHOTOS it holds (a RAW+JPEG pair counts once). */
  sizes: Map<string, number>;
}

export const NO_BURSTS: Bursts = { byFile: new Map(), sizes: new Map() };

export function computeBursts(items: ItemLite[], gapSeconds: number): Bursts {
  const shots = shotsOf(items);
  const out: Bursts = { byFile: new Map(), sizes: new Map() };
  if (shots.length === 0) return out;

  let run: Shot[] = [];
  const flush = () => {
    if (run.length >= MIN_BURST_SIZE) {
      // Keyed by the first shot, so the key is stable as long as the burst is.
      const key = `b${run[0].groupId}`;
      out.sizes.set(key, run.length);
      for (const shot of run) for (const id of shot.ids) out.byFile.set(id, key);
    }
    run = [];
  };

  for (const shot of shots) {
    const prev = run[run.length - 1];
    if (prev) {
      const sameDevice = shot.camera === prev.camera;
      const closeInTime = shot.time - prev.time <= gapSeconds;
      // A hash that will never arrive (the thumbnail could not be decoded)
      // degrades to time-only, so one broken frame cannot split a burst around
      // it. A hash that has merely not arrived YET is a different thing and must
      // not be read as "these look alike": during an import nothing has a hash,
      // and time alone says every photo of a fast shoot is one enormous burst
      // that then breaks apart as the thumbnails land. Waiting shows nothing for
      // a moment; guessing shows something wrong.
      const undecided = shot.pending || prev.pending;
      const looksAlike =
        !undecided &&
        (!shot.phash || !prev.phash || hammingHex(shot.phash, prev.phash) <= MAX_BURST_DISTANCE);
      if (!sameDevice || !closeInTime || !looksAlike) flush();
    }
    run.push(shot);
  }
  flush();
  return out;
}

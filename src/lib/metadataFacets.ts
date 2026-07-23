/**
 * Bucketing of the numeric photographic settings (ISO, aperture, focal length)
 * for the filter panel. A shoot in automatic mode produces dozens of distinct
 * readings, which would flood a chip list — so each value collapses onto a
 * standard photographic step (ISO/aperture) or a focal-range band. The panel
 * offers only the buckets some present photo falls into; both the panel (to
 * build the chips) and the filter predicate (to test a photo) go through the
 * same `*Bucket` functions, so what you click always matches what filters.
 */

export interface FacetBucket {
  /** Stable identity stored in the filter state and persisted. */
  key: string;
  /** Human label shown on the chip. */
  label: string;
}

/** Index of the value in `stops` closest to `v` in log space — equal
 *  photographic distance either side — i.e. the standard step it rounds to. */
function nearestStopIndex(v: number, stops: number[]): number {
  let best = 0;
  let bestDist = Infinity;
  for (let i = 0; i < stops.length; i++) {
    const d = Math.abs(Math.log(v / stops[i]));
    if (d < bestDist) {
      bestDist = d;
      best = i;
    }
  }
  return best;
}

const ISO_STOPS = [100, 200, 400, 800, 1600, 3200, 6400, 12800, 25600, 51200];

export function isoBucket(iso: number | null | undefined): FacetBucket | null {
  if (iso == null || !Number.isFinite(iso) || iso <= 0) return null;
  const i = nearestStopIndex(iso, ISO_STOPS);
  const stop = ISO_STOPS[i];
  // The top stop is open-ended so an ultra-high reading still lands somewhere.
  const label = i === ISO_STOPS.length - 1 ? `${stop}+` : `${stop}`;
  return { key: `iso:${stop}`, label };
}

const APERTURE_STOPS = [1.0, 1.4, 2.0, 2.8, 4.0, 5.6, 8.0, 11, 16, 22, 32];

export function apertureBucket(f: number | null | undefined): FacetBucket | null {
  if (f == null || !Number.isFinite(f) || f <= 0) return null;
  const stop = APERTURE_STOPS[nearestStopIndex(f, APERTURE_STOPS)];
  // Whole-stop apertures print without a decimal (f/8, not f/8.0).
  const shown = Number.isInteger(stop) ? `${stop}` : stop.toFixed(1);
  return { key: `f:${stop}`, label: `f/${shown}` };
}

interface FocalBand {
  key: string;
  label: string;
  /** Inclusive upper edge in mm; the last band is open (Infinity). */
  max: number;
}

const FOCAL_BANDS: FocalBand[] = [
  { key: "focal:0", label: "≤24 mm", max: 24 },
  { key: "focal:24", label: "24–50 mm", max: 50 },
  { key: "focal:50", label: "50–105 mm", max: 105 },
  { key: "focal:105", label: "105–200 mm", max: 200 },
  { key: "focal:200", label: "200–400 mm", max: 400 },
  { key: "focal:400", label: "400 mm+", max: Infinity },
];

export function focalBucket(mm: number | null | undefined): FacetBucket | null {
  if (mm == null || !Number.isFinite(mm) || mm <= 0) return null;
  const band = FOCAL_BANDS.find((b) => mm <= b.max) ?? FOCAL_BANDS[FOCAL_BANDS.length - 1];
  return { key: band.key, label: band.label };
}

// Standard full-stop shutter speeds in seconds, fast to slow. A raw reading
// rounds to the nearest one in log space.
const SHUTTER_STOPS = [
  1 / 8000,
  1 / 4000,
  1 / 2000,
  1 / 1000,
  1 / 500,
  1 / 250,
  1 / 125,
  1 / 60,
  1 / 30,
  1 / 15,
  1 / 8,
  1 / 4,
  1 / 2,
  1,
  2,
  4,
  8,
  15,
  30,
];

/** Camera-style shutter label: whole seconds as "Ns", faster as "1/N". */
function shutterLabel(secs: number): string {
  return secs >= 1 ? `${secs}s` : `1/${Math.round(1 / secs)}`;
}

export function shutterBucket(secs: number | null | undefined): FacetBucket | null {
  if (secs == null || !Number.isFinite(secs) || secs <= 0) return null;
  const label = shutterLabel(SHUTTER_STOPS[nearestStopIndex(secs, SHUTTER_STOPS)]);
  return { key: `sh:${label}`, label };
}

/** All possible buckets in ascending order — the panel filters these down to
 *  the ones actually present, keeping a stable low-to-high chip order. */
export const ISO_BUCKETS: FacetBucket[] = ISO_STOPS.map((s) => isoBucket(s)!);
export const APERTURE_BUCKETS: FacetBucket[] = APERTURE_STOPS.map((s) => apertureBucket(s)!);
export const FOCAL_BUCKETS: FacetBucket[] = FOCAL_BANDS.map((b) => ({ key: b.key, label: b.label }));
export const SHUTTER_BUCKETS: FacetBucket[] = SHUTTER_STOPS.map((s) => shutterBucket(s)!);

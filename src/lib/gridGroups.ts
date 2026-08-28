/**
 * Grouping dimensions for the grid's "Group by" view. Each dimension maps a
 * photo to the bucket (section) it belongs to — the same vocabulary as the Sort
 * & Filter panel, plus Folder. Numeric photographic settings reuse the step
 * buckets from `metadataFacets` so grouping and filtering agree. Grouping is
 * multi-level: the active dimensions are applied in order (level 0 outermost),
 * and `groupCompare` sorts items so members of the same bucket stay contiguous
 * while the existing catalog sort is preserved within the deepest bucket.
 */
import type { ItemLite } from "./api";
import { NO_BURSTS, type Bursts } from "./bursts";
import {
  apertureBucket,
  focalBucket,
  isoBucket,
  metaTextKey,
  shutterBucket,
} from "./metadataFacets";

/** A photo's bucket under one dimension. `sort` orders the buckets: a number
 *  for numeric/temporal dimensions, a string for categorical ones. */
export interface GroupBucket {
  key: string;
  label: string;
  sort: number | string;
}

/** Facts a dimension may need that are not on the item itself. Passed in rather
 *  than imported, because the burst map lives in `session` and this module is
 *  imported *by* it — reaching back would be a cycle. */
export interface GroupContext {
  bursts: Bursts;
}

export const EMPTY_GROUP_CONTEXT: GroupContext = { bursts: NO_BURSTS };

export interface GroupDim {
  key: string;
  label: string;
  of(item: ItemLite, ctx: GroupContext): GroupBucket;
}

/** Items with no value for a dimension collect in one "—" bucket, always last. */
const UNKNOWN: GroupBucket = { key: "~unknown", label: "—", sort: "￿" };

/** Directory portion of a relPath (forward slashes), "" for the project root.
 *  Local copy to keep this module free of a `session` import cycle. */
function dirOf(relPath: string): string {
  const idx = Math.max(relPath.lastIndexOf("/"), relPath.lastIndexOf("\\"));
  return idx === -1 ? "" : relPath.slice(0, idx).replace(/\\/g, "/");
}

/** Bucket for a free-text EXIF string (camera, lens). The key is case-folded so
 *  "SONY ILCE-7M3" and "Sony ILCE-7M3" collect in one section. The label is the
 *  photo's own spelling, which the grid reads from the first photo of the
 *  section: a dimension sees one item at a time, so it cannot know which
 *  spelling is the most common one (the filter panel, which sees the whole
 *  catalogue, does pick that one). */
function textBucket(value: string | null | undefined): GroupBucket {
  if (!value) return UNKNOWN;
  const key = metaTextKey(value);
  return { key, label: value, sort: key };
}

function pad(n: number): string {
  return n < 10 ? `0${n}` : `${n}`;
}

/** The calendar day a photo belongs to, as `YYYY-MM-DD`.
 *
 * capture_time/mtime are UTC unix seconds; the day is formatted in UTC to match
 * the backend's timezone-free convention. Shared by the Date grouping dimension
 * and the date filter, so "group by day" and "filter by this day" can never
 * disagree about where a midnight shot lands. */
export function dayKey(item: ItemLite): string {
  const d = new Date((item.captureTime ?? item.mtime) * 1000);
  return `${d.getUTCFullYear()}-${pad(d.getUTCMonth() + 1)}-${pad(d.getUTCDate())}`;
}

const LABEL_ORDER: Record<string, number> = {
  Red: 0,
  Yellow: 1,
  Green: 2,
  Blue: 3,
  Purple: 4,
};

export const GROUP_DIMS: GroupDim[] = [
  {
    key: "burst",
    label: "Burst",
    of: (i, ctx) => {
      const key = ctx.bursts.byFile.get(i.id);
      // Photos outside any burst share the "—" bucket, which sorts last, so a
      // grid grouped by burst reads as "the bursts, then everything else".
      if (!key) return UNKNOWN;
      const n = ctx.bursts.sizes.get(key) ?? 0;
      return { key, label: `Burst · ${n} shots`, sort: i.captureTime ?? i.mtime };
    },
  },
  {
    key: "date",
    label: "Date",
    of: (i) => {
      const day = dayKey(i);
      return { key: day, label: day, sort: i.captureTime ?? i.mtime };
    },
  },
  {
    key: "folder",
    label: "Folder",
    of: (i) => {
      const dir = dirOf(i.relPath);
      return { key: dir || "~root", label: dir || "(root)", sort: dir };
    },
  },
  {
    key: "camera",
    label: "Camera",
    of: (i) => textBucket(i.camera),
  },
  {
    key: "lens",
    label: "Lens",
    of: (i) => textBucket(i.lens),
  },
  {
    key: "iso",
    label: "ISO",
    of: (i) => {
      const b = isoBucket(i.iso);
      return b ? { key: b.key, label: `ISO ${b.label}`, sort: i.iso ?? 0 } : UNKNOWN;
    },
  },
  {
    key: "aperture",
    label: "Aperture",
    of: (i) => {
      const b = apertureBucket(i.fNumber);
      return b ? { key: b.key, label: b.label, sort: i.fNumber ?? 0 } : UNKNOWN;
    },
  },
  {
    key: "focal",
    label: "Focal length",
    of: (i) => {
      const b = focalBucket(i.focalLength);
      return b ? { key: b.key, label: b.label, sort: i.focalLength ?? 0 } : UNKNOWN;
    },
  },
  {
    key: "shutter",
    label: "Shutter speed",
    of: (i) => {
      const b = shutterBucket(i.exposureTime);
      return b ? { key: b.key, label: b.label, sort: i.exposureTime ?? 0 } : UNKNOWN;
    },
  },
  {
    key: "rating",
    label: "Rating",
    of: (i) => ({
      key: `r${i.rating}`,
      label: i.rating > 0 ? "★".repeat(i.rating) : "Unrated",
      sort: -i.rating,
    }),
  },
  {
    key: "flag",
    label: "Flag",
    of: (i) => {
      if (i.flag === 1) return { key: "pick", label: "Picks", sort: 0 };
      if (i.flag === -1) return { key: "reject", label: "Rejects", sort: 2 };
      return { key: "unflagged", label: "Unflagged", sort: 1 };
    },
  },
  {
    key: "label",
    label: "Color label",
    of: (i) =>
      i.label ? { key: i.label, label: i.label, sort: LABEL_ORDER[i.label] ?? 9 } : UNKNOWN,
  },
  {
    key: "type",
    label: "File type",
    of: (i) => {
      const isPair = i.groupSize > 1 && !i.decoupled;
      if (isPair) return { key: "rawjpeg", label: "RAW+JPEG", sort: 0 };
      if (i.kind === 0) return { key: "raw", label: "RAW", sort: 1 };
      if (i.kind === 2) return { key: "video", label: "Video", sort: 3 };
      return { key: "jpeg", label: "JPEG", sort: 2 };
    },
  },
  {
    key: "ext",
    label: "Extension",
    of: (i) => {
      const e = i.ext.toLowerCase();
      return e ? { key: e, label: e.toUpperCase(), sort: e } : UNKNOWN;
    },
  },
  {
    key: "orientation",
    label: "Orientation",
    of: (i) => {
      if (i.width == null || i.height == null) return UNKNOWN;
      const rotated = i.orientation != null && i.orientation >= 5 && i.orientation <= 8;
      const w = rotated ? i.height : i.width;
      const h = rotated ? i.width : i.height;
      if (h > w) return { key: "portrait", label: "Portrait", sort: 0 };
      if (w > h) return { key: "landscape", label: "Landscape", sort: 1 };
      return { key: "square", label: "Square", sort: 2 };
    },
  },
];

const DIM_BY_KEY = new Map(GROUP_DIMS.map((d) => [d.key, d]));

export function groupDim(key: string): GroupDim | undefined {
  return DIM_BY_KEY.get(key);
}

/** The bucket a photo falls into for a given dimension key (or null for an
 *  unknown dimension). Used by the grid to detect section boundaries + labels. */
export function bucketOf(
  item: ItemLite,
  dimKey: string,
  ctx: GroupContext = EMPTY_GROUP_CONTEXT,
): GroupBucket | null {
  return DIM_BY_KEY.get(dimKey)?.of(item, ctx) ?? null;
}

/** Order two buckets: the "unknown" sentinel always sorts last, then by `sort`
 *  (numbers numerically, strings by locale). */
function cmpBucket(a: GroupBucket, b: GroupBucket): number {
  // Same bucket at this level → equal, so groupCompare falls through to the
  // next grouping level (or, at the deepest one, preserves the catalog sort).
  // Essential because several dimensions carry a PER-ITEM `sort` value rather
  // than a per-bucket one — date sorts by the item's capture_time, ISO/aperture/
  // focal/shutter by the raw reading — so two items of the SAME bucket have
  // different `sort`s. Without this short-circuit their non-zero comparison
  // would make groupCompare return early and never reach the inner level,
  // leaving sub-groups interleaved (e.g. Date→ISO would stay in pure time order,
  // scattering each ISO across the day) instead of contiguous.
  if (a.key === b.key) return 0;
  const au = a.key === UNKNOWN.key;
  const bu = b.key === UNKNOWN.key;
  if (au !== bu) return au ? 1 : -1;
  if (typeof a.sort === "number" && typeof b.sort === "number") return a.sort - b.sort;
  return String(a.sort).localeCompare(String(b.sort));
}

/** Stable multi-level comparator: compare each active dimension in order; the
 *  first that differs decides. Equal on every level → 0, so the array's existing
 *  order (the active catalog sort) is preserved within the deepest bucket. */
export function groupCompare(
  a: ItemLite,
  b: ItemLite,
  dimKeys: string[],
  ctx: GroupContext = EMPTY_GROUP_CONTEXT,
): number {
  for (const key of dimKeys) {
    const dim = DIM_BY_KEY.get(key);
    if (!dim) continue;
    const c = cmpBucket(dim.of(a, ctx), dim.of(b, ctx));
    if (c !== 0) return c;
  }
  return 0;
}

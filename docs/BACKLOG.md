# Cullant — working backlog (post-0.7.2)

The stream planned here is **done**: search, keyboard selection, a shrunk
toolbar, rotation, undo, XMP import, bursts and survey all landed. What follows
is kept as the record of what was decided and why, plus what is still open.

## What landed

| # | Item | Commits |
|---|---|---|
| A | Filename search — `Ctrl+F` overlay and a Sort & Filter field over one shared `nameFilter` | `feat(search)` |
| B | Keyboard range selection, deselect all, invert | `feat(selection)` |
| C | Toolbar shrunk from seven controls to five | 4 × `refactor/feat(toolbar,settings)` |
| D | Rotate with `[` / `]`, exported as `tiff:Orientation` | `fix(thumbs)`, `feat(culling)`, `feat(xmp)` |
| E | Undo and commit history, inside the commit dialog | `fix(commit)`, `feat(undo)`, `feat(commit)` |
| F | Import ratings from existing XMP sidecars | `feat(scan)` |
| G | Burst detection: pHash in the ingest, grid grouping, loupe badge | `feat(thumbs)`, `feat(bursts)` |
| H | Survey (N-up elimination) | `feat(survey)` |

## Decisions worth not re-litigating

| Topic | Decision |
|---|---|
| Toolbar budget | The top toolbar is full. New surfaces go inside a menu that already exists — history lives in the commit dialog, survey in the selection bar |
| Search entry points | Two entry points, one shared filter; the overlay is a remappable command, not a hard-coded key |
| Rotation | Rewrites `files.orientation` and never touches the file. `tiff:Orientation` is exported **even at its neutral value**, because a sidecar's job is to override the file's own EXIF |
| Undo scope | Follows from how the commit disposed of the file. Permanent deletes and XMP writes are not reversible and say so in place |
| Undo safety | Restoring never overwrites: an occupied path gets the same `.N` suffixing the trash uses |
| XMP conflict | Newer wins, `state_updated_at` against the sidecar mtime, reported once per scan |
| XMP import | Never sets `xmp_dirty` — that would queue a write-back of what was just read |
| Burst threshold | Fixed gap or adaptive; adaptive declines rather than guessing when the intervals are not bimodal, and shows the value it settled on |
| Burst invariants | Never spans two cameras; never splits a RAW+JPEG pair; a missing pHash never splits a burst |
| Survey | Opening it drops the selection, because `targets()` prefers a selection over the focus and every reject would otherwise hit all N |

## Known limits

- **`capture_time` is whole seconds.** A 10 fps burst reports gaps of 0, so no
  threshold can distinguish frames *inside* one burst. Separating bursts from
  each other works fine.
- **Two identical camera bodies report the same EXIF string**, so the
  device partition cannot tell them apart. The serial number is not in the
  schema.
- **A pair takes its shared sidecar on both halves** on import, *while the two
  agree*. Lightroom strictly applies `IMG.xmp` to the RAW only, but being
  asymmetric would break the round-trip of our own export.
- **A pair that disagrees gets a sidecar each.** One sidecar cannot carry two
  states, and the old rule — the higher-id row wins — silently discarded one of
  them. Diverging is a legitimate way to work (queue the RAWs, keep the JPEGs),
  so nothing is thrown away: the group's primary keeps `IMG.xmp`, the name other
  applications look for, and the other half is exported to `IMG.JPG.xmp`, the
  form Bridge and exiftool use for non-raw files. Import prefers a file's own
  sidecar over the shared one, or a rescan would undo the difference; and the
  per-file copy is deleted when the pair agrees again, or when that half is.

## Still open

- **`dc:subject` keywords → task tags**, in both directions. Needs tag creation
  and name matching; deliberately left out of the XMP import.
- **No frontend test harness.** The backend has 76 tests; the frontend has
  none. `src/lib/bursts.ts` was verified by transpiling it with esbuild and
  exercising it outside the repo — worth making permanent with vitest.
- **Cold-import numbers (2026-07-28), same build, same corpus.** The ingest now
  reports its own storage counters (`adb logcat -s cullant` on device), so this
  is measured rather than inferred. 600 synthetic JPEGs: **1800 → 1260 source
  reads**. 60 RAW+JPEG pairs: **360 → 180**, and every read is the small JPEG
  half — a paired RAW is not opened at all until someone zooms in. Desktop wall
  clock is a wash (74 s vs 70 s): a memory-mapped read costs almost nothing
  there, and the fused pass re-renders the lead window's thumbnails. The reads
  are the point on Android SAF, which cannot memory-map at all.
- **Ingest benchmark is not a same-session A/B.** With pHash: 600 files in
  5.15 s, against the ~7.44 s recorded in `backend-optimization-audit.md`. No
  regression is apparent, but each release build costs ~10 minutes so the two
  numbers come from different runs.
- **Rules engine** ("rating ≥ 5 → move to /selects"): `pending_actions.origin`
  still reserves `1 = rule` and nothing writes it.

## Working agreements

- One logical change per commit; no `Co-Authored-By` trailers.
- Before each commit: `cargo test`, `cargo clippy --all-targets -- -D warnings`,
  `npm run check`. Run the app for anything with runtime surface.

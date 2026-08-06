# Open items

Only what is still open. The records of work that landed are in git history —
`git log`, and `git show d54fdd6:docs/PLAN.md` for the original plan.

## Decoding

- **Olympus / OM System RAWs do not render.** The preview is in the MakerNote
  and `raw.rs` never looks there. See [formats.md](formats.md#known-gaps).
- **A RAW with no usable preview is tombstoned.** Older Panasonic bodies write
  no embedded JPEG at all, so the cell is blank forever even though `rawler`
  could demosaic it. Deciding to demosaic on demand is the open question — it is
  seconds per frame, so it cannot be the pregeneration path.
- **iOS rung of the HEIF ladder** (ImageIO). Drops in beside WIC; nothing else
  changes.

## Testing

- **No frontend test harness.** The backend has 122 tests; the frontend has
  none. `src/lib/bursts.ts` was once verified by transpiling it with esbuild and
  running it outside the repo — worth making permanent with vitest.
- **No video corpus, and no RAW+JPEG pair in the fixture corpus.** See
  [testing.md](testing.md#what-is-not-tested).

## Features

- **Rules engine** ("rating ≥ 5 → move to /selects"). `pending_actions.origin`
  reserves `1 = rule` and nothing writes it.
- **`dc:subject` keywords ↔ task tags**, both directions. Needs tag creation and
  name matching; deliberately left out of the XMP import.
- **M8 polish**: dedicated settings pane, move/copy rules UI, commit
  history/undo UI, installer signing.

## Known limitations, decided rather than open

- **Two identical camera bodies report the same EXIF string**, so the device
  filter cannot tell them apart. The serial number is not in the schema.
- **A pair takes its shared sidecar on both halves** on import, while the two
  agree. Lightroom applies `IMG.xmp` to the RAW only, but being asymmetric would
  break the round-trip of Cullant's own export.
- **Benchmarks are not same-session A/B.** Each release build costs ~10 minutes,
  so recorded numbers come from different runs and are indicative only.

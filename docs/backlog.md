# Open items

This list holds only what is open. Git history holds the record of finished
work. Run `git log`, or `git show d54fdd6:docs/PLAN.md` to read the original
plan.

## Decoding

- **The app tombstones a RAW that holds no usable preview.** Older Panasonic
  bodies write no embedded JPEG, so the cell stays empty although `rawler` could
  demosaic the file. The open question is when to demosaic. One frame takes
  seconds, so pregeneration cannot do it.
- **The iOS rung of the HEIF ladder is missing.** It fits beside the WIC rung.
  Nothing else changes.

## Testing

- **The frontend has no test harness.** The backend has 122 tests. One earlier
  check transpiled `src/lib/bursts.ts` with esbuild and ran it outside the
  repository. Make that permanent with `vitest`.
- **The fixture corpus holds no video and no RAW+JPEG pair.** See
  [testing.md](testing.md#what-the-tests-do-not-cover).

## Features

- **Rules engine.** An example rule: rating 5 or higher moves the file to
  `/selects`. `pending_actions.origin` reserves the value `1` for a rule, and no
  code writes it.
- **Map `dc:subject` keywords to task tags, in both directions.** This needs tag
  creation and name matching. The XMP import leaves it out on purpose.
- **M8 polish.** A dedicated settings pane, a UI for move and copy rules, a UI
  for commit history and undo, and installer signing.

## Limitations that are decided, not open

- **Two identical camera bodies write the same EXIF string.** The device filter
  cannot tell them apart. The schema holds no serial number.
- **A pair takes its shared sidecar onto both halves at import, while the two
  states agree.** Lightroom applies `IMG.xmp` to the RAW only. Cullant does not
  copy that rule, because an asymmetric import would break the round-trip of
  Cullant's own export.
- **The benchmarks compare separate runs.** Each release build costs about 10
  minutes. Treat the recorded numbers as indications, not measurements.

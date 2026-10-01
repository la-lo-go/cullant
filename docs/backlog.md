# Tasks

This list holds open work and the latest task checklist. Git history holds older completed work.
Run `git log`, or `git show d54fdd6:docs/PLAN.md` to read the original plan.

Known defects from the 2026-09-23 source audit live in
[`audit/`](audit/README.md). Read the audit before you start a fix.

## Decoding

- **The app tombstones a RAW that holds no usable preview.** Older Panasonic
  bodies write no embedded JPEG, so the cell stays empty although `rawler` could
  demosaic the file. The open question is when to demosaic. One frame takes
  seconds, so pregeneration cannot do it.
- **The iOS rung of the HEIF ladder is missing.** It fits beside the WIC rung.
  Nothing else changes.

## Testing

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

## Task checklist, 2026-10-01

- [x] Add repeatable preview cache and UI E2E checks. See [testing.md](testing.md).
- [x] Put date filters below Sort and above File name. Put Today and Last 7 days first in the list.
- [x] Hide folder branch actions when the branch already has that state.
- [x] Use the theme hover fill without a focus border.
- [x] Use rounded color squares and cyan input focus for color label names.
- [x] Clear control focus after touch input in Settings.
- [x] Bump to 0.11.1 and build the signed Android ARM64 APK.
- [x] Install 0.11.1 on the connected phone through ADB.
- [x] Replace Compare text badges with small marks. Show zoom controls only for photos.
- [x] Remove the Search button from the toolbar. Keep file name search in Sort & Filter.
- [x] Rename Auto-advance to Auto. Add Auto to the bar layout settings.
- [x] Show Move / Copy as an icon. Add hover, keyboard, and touch hold help to bar buttons.
- [x] Check the updated action bar with 8 new E2E cases and 12 Settings and filter cases.
- [x] Build the updated Android APK.
- [x] Install the updated Android APK through ADB. Check the toolbar and touch hold help on the phone.
- [x] Restore the theme hover color. Use existing theme colors in button help.
- [x] Add theme hover feedback to action bar buttons and view controls, including selected buttons.
- [x] Use Fit whole photo as the default. Keep saved framing choices.
- [x] Check hover and framing changes with 25 E2E cases. Build and install the Android APK. Check Fit and touch hold help on the phone.

## Limitations that are decided, not open

- **Two identical camera bodies write the same EXIF string.** The device filter
  cannot tell them apart. The schema holds no serial number.
- **A pair takes its shared sidecar onto both halves at import, while the two
  states agree.** Lightroom applies `IMG.xmp` to the RAW only. Cullant does not
  copy that rule, because an asymmetric import would break the round-trip of
  Cullant's own export.
- **The benchmarks compare separate runs.** Each release build costs about 10
  minutes. Treat the recorded numbers as indications, not measurements.

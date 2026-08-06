# Cullant

**Fast, keyboard-first photo culling for Windows.**

Load a project folder — subfolders included — flick through instant previews,
and rate, flag, label and queue actions entirely from the keyboard. Nothing
touches your files until you say so: every decision lives in a local SQLite
database and is executed in one reviewed **commit** step, or immediately if you
prefer auto mode.

> ⚠️ Early development. Not yet usable.

## Why another culling tool?

- **Mirror mode is the headline.** Files sharing a directory and basename are
  *one* photo. Rate one, both are rated; delete one, both go. Flip a switch and
  they are independent again, or decouple a single shot to keep the JPEG and
  drop the RAW.

  A shot is not always two files, and Cullant does not assume it is: OM System
  writes `ORF`+`ORI`+`JPG` in Live ND, and RAW+HEIF cameras add a `.HIF`.
  Grouping is N-ary, so rejecting a shot takes every file of it — the alternative
  is an orphan the user never knew existed.

- **Speed first.** Previews come from the camera's own embedded JPEG (the Photo
  Mechanic trick), served straight to the UI through a custom protocol. No
  import step, no waiting for a demosaic.

- **Keyboard everything.** Lightroom-style defaults (`P`/`X`/`U`, `1–5`, `6–9`,
  Caps Lock auto-advance), every binding remappable, plus custom task tags
  ("retouch", "trim", "stabilize"…) with their own shortcuts.

- **Deferred, reviewable actions.** Deletes, moves and copies queue up and run
  in one confirmed commit, with history and undo. Deletion goes to the Recycle
  Bin by default; permanent and `_trash` folder are also available.

- **Plays well with others.** Optional XMP sidecars readable by Lightroom
  Classic and Capture One.

- Basic **video culling** (MP4/MOV) in a separate tab, same workflow.

## Formats

Every mainstream RAW format, plus HEIF and the usual images and video. The full
matrix — including which files render and which only get catalogued, and why the
two are separate — is in [docs/formats.md](docs/formats.md).

Two things worth knowing up front:

- **HEIF metadata always works**, so an iPhone library sorts by real capture
  time and fills every filter facet. The pixels need a decoder borrowed from the
  platform (WIC, Android `ImageDecoder`, or `ffmpeg` 7.0+), because Cullant has
  no HEVC decoder of its own and will not grow a C dependency to get one.
- **Olympus and OM System RAWs currently show blank cells.** The preview is in
  the MakerNote and the code does not look there yet. Metadata reads fine.

## Shortcuts

All remappable via the ⌨ dialog.

`P`/`X`/`U` flags · `1-5` stars · `6-9`/`-` color labels · `T`+`1-9` task tags ·
`←→↑↓` navigate · Caps Lock auto-advance (Shift inverts) · `G`/`E`/`C`
grid/loupe/compare · `Z`/`Space` 100% zoom · `J` RAW↔JPEG · `Ctrl+J` decouple ·
`M` mirror mode · `Del` / `Alt+Del` / `Shift+Del` queue delete (group / RAW only
/ JPEG only) · `Ctrl+Enter` commit · `\` filter bar

## Status

M0–M7 are done, plus two UX polish rounds and HEIF support. M8 is polish:
settings pane, move/copy rules UI, commit history/undo UI, installer signing.

What is still open, and what is a known limitation rather than an oversight, is
in [docs/backlog.md](docs/backlog.md).

Post-MVP: AI-assisted culling (similar-shot grouping, best-of-burst
suggestions), focus peaking, face zoom.

## Development

Prerequisites: Rust (stable) with the MSVC toolchain, Node 20+, npm.

```sh
npm install
npm run tauri dev
```

The frontend is SvelteKit (static adapter) in `src/`; the Rust backend is in
`src-tauri/`. Project state lives in `<project folder>/.cullant/cullant.db`.

Before committing:

```sh
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
cd .. && npm run check
```

## Documentation

| | |
|---|---|
| [docs/testing.md](docs/testing.md) | The three kinds of test data, what each cannot tell you, and the checks that need hardware |
| [docs/formats.md](docs/formats.md) | What is catalogued, what renders, and the grouping rules |
| [docs/building.md](docs/building.md) | Windows and Android builds, signing, size tradeoffs |
| [docs/backlog.md](docs/backlog.md) | What is open |
| [fixtures/README.md](fixtures/README.md) | The corpus of real camera files, and what it found |
| [CLAUDE.md](CLAUDE.md) | Working context: architecture, conventions, gotchas |

## License

[GPL-3.0-or-later](LICENSE). RAW decoding uses
[rawler](https://github.com/dnglab/dnglab) (LGPL-2.1); the test corpus is CC0.

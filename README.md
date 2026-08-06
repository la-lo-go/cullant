# Cullant

**Fast, keyboard-first photo culling for Windows.**

Open a project folder, subfolders included. Move through the previews at speed.
Rate, flag, label and queue actions entirely from the keyboard.

Nothing touches your files until you say so. Cullant records every decision in a
local SQLite database, then carries them out in one **commit** step that you
review first. Auto mode carries them out immediately instead.

> ⚠️ Early development. Not yet usable.

## Why another culling tool?

- **Mirror mode is the headline.** Files that share a directory and a basename
  are *one* photo. Rate one and you rate both. Delete one and both go. Flip a
  switch to make them independent again, or decouple a single shot to keep the
  JPEG and drop the RAW.

  A shot is not always two files, and Cullant does not assume it is. OM System
  writes `ORF`+`ORI`+`JPG` in Live ND, and a RAW+HEIF camera adds a `.HIF`.
  Grouping is N-ary, so rejecting a shot takes every file of it. Otherwise one
  file survives on disk, and the user never learns it is there.

- **Speed first.** Each preview comes from the JPEG that the camera embedded in
  the file — the Photo Mechanic method. A custom protocol serves it straight to
  the interface. There is no import step, and no wait for a demosaic.

- **Keyboard everything.** The defaults follow Lightroom: `P`/`X`/`U`, `1–5`,
  `6–9`, and Caps Lock to advance. Every binding is remappable. Custom task tags
  ("retouch", "trim", "stabilize") get their own shortcuts.

- **Deferred, reviewable actions.** Deletes, moves and copies wait in a queue,
  then run in one commit that you confirm. History and undo cover them. A delete
  goes to the Recycle Bin by default; permanent deletion and a `_trash` folder
  are also available.

- **Works alongside other tools.** Optional XMP sidecars, which Lightroom
  Classic and Capture One both read.

- Basic **video culling** for MP4 and MOV, in a separate tab, with the same
  workflow.

## Formats

Cullant reads every mainstream RAW format, plus HEIF, the common image formats
and video. [docs/formats.md](docs/formats.md) holds the full matrix. It also
explains which files render, which files the app only catalogues, and why those
two sets differ.

One point to know before you start:

- **HEIF metadata always works.** An iPhone library therefore sorts by real
  capture time and fills every filter facet. The pixels need a decoder borrowed
  from the platform: WIC, Android `ImageDecoder`, or `ffmpeg` 7.0 or later.
  Cullant has no HEVC decoder of its own, and it will not add a C dependency to
  get one.

## Shortcuts

You can remap all of these in the ⌨ dialog.

`P`/`X`/`U` flags · `1-5` stars · `6-9`/`-` colour labels · `T`+`1-9` task tags ·
`←→↑↓` navigate · Caps Lock auto-advance (Shift inverts) · `G`/`E`/`C`
grid/loupe/compare · `Z`/`Space` 100% zoom · `J` RAW↔JPEG · `Ctrl+J` decouple ·
`M` mirror mode · `Del` / `Alt+Del` / `Shift+Del` queue a delete (group / RAW
only / JPEG only) · `Ctrl+Enter` commit · `\` filter bar

## Status

Milestones M0 to M7 are complete, plus two UX passes and HEIF support. M8 is
polish: a settings pane, a UI for move and copy rules, a UI for commit history
and undo, and installer signing.

[docs/backlog.md](docs/backlog.md) lists what is open, and which limitations the
project decided to accept.

After the MVP: AI-assisted culling (grouping of similar shots, best-of-burst
suggestions), focus peaking, and face zoom.

## Development

You need Rust (stable) with the MSVC toolchain, Node 20 or later, and npm.

```sh
npm install
npm run tauri dev
```

The frontend is SvelteKit with the static adapter, in `src/`. The Rust backend is
in `src-tauri/`. Each project keeps its state in
`<project folder>/.cullant/cullant.db`.

Run these checks before every commit:

```sh
cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test
cd .. && npm run check
```

## Documentation

| Document | Contents |
|---|---|
| [docs/testing.md](docs/testing.md) | The three kinds of test data, what each one cannot prove, and the checks that need hardware |
| [docs/formats.md](docs/formats.md) | What the app catalogues, what it renders, and the grouping rules |
| [docs/building.md](docs/building.md) | Windows and Android builds, signing, and size tradeoffs |
| [docs/backlog.md](docs/backlog.md) | What is open |
| [fixtures/README.md](fixtures/README.md) | The corpus of real camera files, and the defects it found |
| [CLAUDE.md](CLAUDE.md) | Working context: architecture, conventions and traps |

## Licence

[GPL-3.0-or-later](LICENSE). RAW decoding uses
[rawler](https://github.com/dnglab/dnglab), which is LGPL-2.1. The test corpus is
CC0.

# Cullant

**Fast, keyboard-first photo culling for Windows.**

Cullant is an open-source desktop app for going through a shoot fast: load a project folder (subfolders included), flick through instant previews, and rate / flag / label / queue actions entirely from the keyboard. Nothing touches your files until you say so — every decision is stored in a local SQLite database and executed in a single reviewed **commit** step (or immediately, if you prefer auto mode).

> ⚠️ Early development. Not yet usable.

## Why another culling tool?

- **RAW+JPEG mirror mode** — the headline feature. Files sharing a basename (`IMG_0421.CR3` + `IMG_0421.JPG`) are treated as *one* photo. Rate one, both are rated; delete one, both go. Flip a switch and they're independent again. Decouple a single pair when you want to keep the JPEG but drop the RAW.
- **Speed first** — previews come from the camera's embedded JPEG (the Photo Mechanic trick), served straight to the UI through a custom protocol. No import step, no waiting for RAW demosaic.
- **Keyboard everything** — Lightroom-style defaults (`P`/`X`/`U`, `1–5`, `6–9`, Caps Lock auto-advance), every binding remappable, plus custom task tags ("retouch", "trim", "stabilize"…) with their own shortcuts.
- **Deferred, reviewable actions** — deletes, moves and copies queue up and run in one confirmed commit, with history and undo. Deletion goes to the Recycle Bin by default (permanent or `_trash` folder also available).
- **Plays well with others** — optional XMP sidecars readable by Lightroom Classic and Capture One.
- Basic **video culling** (MP4/MOV) in a separate tab, same workflow.

## Status / roadmap

- [x] M0 — scaffold: Tauri v2 + Svelte 5, SQLite schema, `cullant://` protocol, folder picker
- [x] M1 — recursive scan, thumbnail pipeline, virtualized grid
- [x] M2 — culling state, keyboard engine, filters
- [x] M3 — viewer, filmstrip, compare, 100% zoom
- [x] M4 — RAW+JPEG mirror mode (decouple/recouple, J/Ctrl+J)
- [x] M5 — task tags (custom, scoped, T+digit chord)
- [x] M6 — pending actions, commit dialog, XMP sidecars, deletion modes
- [x] M7 — video tab (Range-streamed playback)
- [ ] M8 — polish: settings pane, move/copy rules UI, commit history/undo UI, installer signing, real-RAW test pass

Default shortcuts (all remappable via the ⌨ dialog): `P`/`X`/`U` flags · `1-5` stars ·
`6-9`/`-` color labels · `T`+`1-9` task tags · `←→↑↓` navigate · Caps Lock auto-advance
(Shift inverts) · `G`/`E`/`C` grid/loupe/compare · `Z`/`Space` 100% zoom · `J` RAW↔JPEG ·
`Ctrl+J` decouple pair · `M` mirror mode · `Del` / `Alt+Del` / `Shift+Del` queue delete
(pair / RAW only / JPEG only) · `Ctrl+Enter` commit · `\` filter bar

Post-MVP: AI-assisted culling (similar-shot grouping, best-of-burst suggestions), HEIC, focus peaking, face zoom.

## Development

Prerequisites: Rust (stable), Node 20+, npm.

```sh
npm install
npm run tauri dev
```

The frontend is SvelteKit (static adapter) in `src/`; the Rust backend is in `src-tauri/`. Project state lives in `<project folder>/.cullant/cullant.db`.

## License

[GPL-3.0-or-later](LICENSE). RAW decoding will use [rawler](https://github.com/dnglab/dnglab) (LGPL-2.1).

# Cullant — working backlog (post-0.7.2)

Reference for the current work stream. Ordered by ROI against the mission:
**cull thousands of photos fast, from the keyboard, without fear.**

Status legend: `TODO` · `IN PROGRESS` · `DONE` · `OPEN QUESTION` (blocks start)

---

## 1. Undo / commit history — `TODO`

**Why it is first.** The commit is currently a cliff. Nothing can be reversed.
This is the trust gap that makes a user hesitate before the last step of every
session.

**What already exists.** The hard half is written and unused:

- `commits` and `commit_entries` tables — `db/migrations.rs:84`, `:91`.
- Every commit opens a `commits` row (`engine/committer.rs:683`) and closes it
  with a status (`:726`).
- Every file operation appends a `commit_entries` row through `record()`
  (`engine/committer.rs:369`), with `before_path`, `after_path`, `undo_info`,
  `result` and `error`.
- `undo_info` already carries what an undo needs: the project-relative `_trash`
  path for deletes (`store_trash`, `:298`) and the destination for moves
  (`:296`).

**What is missing.** No reader. There is no `list_commits`, no `undo_commit`,
no `undo_commit_entry`, and no UI. The audit trail is written and thrown away.

**Scope.**

- Backend: a read module over `commits` + `commit_entries`, plus undo execution
  per entry and per commit.
- Undo semantics per action:
  - move / copy → move the file back to `before_path`; clear the row.
  - delete in `_trash` mode → move it back from the trash path. Reversible.
  - delete in permanent mode → NOT reversible. Show it as such; never promise.
  - XMP write → out of scope for undo (the sidecar is an export layer).
- Re-ingest the restored rows so `files.status` returns to `0=present`.
- Frontend: a history dialog listing commits, expandable to entries, with
  per-entry and per-commit undo.

**Risks.** A restore must not overwrite a file that now occupies the old path.
Reuse the `.N` collision suffixing that `store_trash` already applies.

---

## 2. Import XMP — read sidecars on scan — `TODO`

**Why.** `engine/xmp.rs` is write-only. `scan/` never mentions xmp. A folder
already rated in Lightroom, Bridge or FastRawViewer opens blank in Cullant.
That breaks the most common onboarding path (the photographer arrives with
previous work) and breaks the round trip (cull here → adjust in LR → rescan).

**What already exists.** Reading is strictly simpler than the merge that
already works. `merge_into_existing` (`engine/xmp.rs:118`) already parses the
`rdf:Description` attributes with quick-xml; `patch_description` (`:159`)
already knows every field Cullant owns.

**Fields to read.** The same set the writer owns:

- `xmp:Rating` → `files.rating`
- `xmp:Label` → `files.label` (English LR strings; localized LR writes
  translated strings — document the limit)
- `xmpDM:pick` / `xmpDM:good` → `files.flag`
- `dc:subject` → task tags
- `cullant:` namespace → lossless round trip of our own state

**Sidecar ownership.** A sidecar belongs to the RAW, never to the JPEG of a
pair (see PLAN.md "Interop XMP"). Import must follow the same rule.

**Conflict policy — decide before coding.** When the DB row and the sidecar
disagree:

- On first import of a project that has no culling state yet, the sidecar wins.
  This is the onboarding case and it needs no prompt.
- On a later rescan, prefer the newer of `files.state_updated_at` and the
  sidecar mtime, and report the count once per scan instead of prompting per
  file.

**Careful.** Importing must not mark the file `xmp_dirty`. Otherwise every scan
would queue a write-back of what was just read.

---

## 3. Filename search — `TODO`

**Why.** There is no way to find a photo by name. Not in `FiltersPanel`, not in
`session`. With 5k+ files and a folder tree, "where is IMG_0421" has no answer.

**Cost.** Near zero. The whole index already lives in the client, so this is an
input plus a substring predicate in the `session.filtered` chain.

**Two entry points, one state.** Build both, over a single `nameFilter` in
`session`:

- A keyboard overlay on `Ctrl+F`. It floats over the grid, filters live while
  you type, `Esc` closes it and `Enter` jumps to the first hit.
- A `Name` field in the Sort & Filter panel, beside camera and lens.

Both write the same store field, so opening one shows what the other typed.
Clearing must be reachable from both.

**Before coding.** `ctrl+f` is free in `DEFAULT_BINDINGS` (`f` alone is
`view.fullscreen`). Register the overlay as a real remappable command, not a
hard-coded key. Verify WebView2 does not swallow `Ctrl+F` for its own find bar.

---

## 4. Burst grouping — `TODO`

**Why.** `file_analysis` and `similarity_clusters` exist in the schema and are
completely empty. Picking the best frame of a burst is the most repetitive act
in culling, and it is what Aftershoot and Narrative sell.

**Why the cheap half is nearly free.** Time-window grouping needs no new decode
pass and no new column:

- `capture_time` is already populated and indexed (`idx_files_time`).
- `gridGroups.ts` already supports multi-level grouping with collapsible
  sections and per-section select-all.

So the first version is a grouping key plus a grid grouping mode. pHash comes
later, computed over the thumbnail that already gets generated.

**Interaction with pairs.** A RAW+JPEG pair is one logical photo. Burst
grouping must operate on groups, never split a pair across buckets.

**Decisions.** See "Resolved decisions" below.

---

## Flecos (smaller gaps)

### F1. Keyboard range selection — `TODO`

`session.rangeSelect()` exists (`stores/session.svelte.ts:678`) but only the
mouse reaches it. The dispatcher treats Shift purely as the auto-advance
inverter (`keyboard/dispatcher.svelte.ts:208`). In a keyboard-first app,
Shift+Arrow to extend a selection is a baseline expectation.

**Care.** Shift must keep its auto-advance meaning on classification keys.
Scope the range binding to navigation keys, and keep an explicit selection
anchor so extending, then reversing direction, shrinks the range.

Also missing: clear selection and invert selection as commands. Only
`select.all` exists.

### F2. Deletion mode and XMP settings are not in Settings — `TODO`

`deletionMode` is only reachable from inside `CommitDialog`
(`components/CommitDialog.svelte:527`). `SettingsDialog` has eight cards and
none for deletion or for XMP. Move or mirror the control into Settings, and add
the XMP behaviour there once item 2 lands.

The PLAN's "auto vs deferred per action type" map was never built. Everything
is deferred today. Decide whether to build it or drop it from the plan.

### F3. Rotate (`[` / `]`) — `TODO`

In the PLAN default keymap, absent from `CommandId`. The `orientation` column
already exists. Needs a write path plus a re-render of thumb and preview.

### F4. Survey view (N-up) — `TODO`

The PLAN reserves `N`. Only 2-up compare exists.

Survey shows N selected photos at once, fitted to the screen. Rejecting one
removes it from the survey and the rest grow. Repeat until the winner remains.

- Compare (2-up) = fine A/B, decide which of two is sharper.
- Survey (N-up) = bulk elimination, reduce a burst of 8 to 1.

It is the natural companion of item 4: group the burst → open it in survey →
eliminate until one remains.

---

## Working notes

- Every item ships as its own atomic commit, or a short series of them.
- Before each commit: `cargo test`, `cargo clippy --all-targets -- -D warnings`,
  `npm run check`. Run the app for anything with runtime surface.
- Prefer `npm run tauri dev` over a standalone `cargo build` to verify backend
  changes.

## Resolved decisions

| Topic | Decision |
|---|---|
| Delivery order | Quick wins first, then the big pieces: A search · B range select · C toolbar · D rotate · E undo · F XMP import · G bursts · H survey |
| Search entry points | Both a `Ctrl+F` overlay (a real remappable command) and a `Name` field in Sort & Filter, over one shared `nameFilter` |
| Burst v1 | Time window + pHash from the start — time proposes, pHash confirms |
| Burst UX | Both a `Group by ▸ Burst` grid mode and a `3/8` badge with in-set navigation in the loupe. The View-menu entry appears only when the project contains bursts |
| Burst threshold | Offer both adaptive-per-session and a fixed configurable gap, selectable in Settings |
| Burst device split | A burst must never span two devices — partition by camera before the time window |
| XMP conflict | Newer wins: compare `state_updated_at` against the sidecar mtime, report once per scan with a count |
| Toolbar budget | The top toolbar is full. No new top-level buttons — new surfaces go inside a menu that already exists |

## Standing constraint — the top toolbar is at capacity

`toolbar-right` (`src/routes/+page.svelte:355-425`) already carries up to seven
controls: Mirror/Separate, Sort & Filter, View, the action-bar toggle, Task
tags, Settings and Commit. On a phone held in portrait that does not fit.

Nothing new goes in the toolbar. The commit history therefore lives inside
`CommitDialog`, and survey is reached from the keyboard and the selection bar.

### F5. Shrink the toolbar — `TODO`

Four moves that take `toolbar-right` from seven controls to five:

- Move **Mirror / Separate** into the View panel as its own section. Keep the
  state visible by folding a non-default mirror mode into the View button's
  `hasCustomView` indicator (`+page.svelte:214`) — it changes what every grid
  cell means and must not go silent.
- Move **Task tags** into Settings, as an "Edit task tags…" row matching the
  existing "Keyboard shortcuts…" row (`SettingsDialog.svelte:309-317`).
- Give the **View** button a text label; it is icon-only while its neighbour
  Sort & Filter is labelled.
- Add the deletion-mode control to Settings (see F2).

The full implementation plan for this stream lives at
`C:\Users\lalop\.claude\plans\replicated-churning-sketch.md`.

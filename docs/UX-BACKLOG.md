# Cullant — UX / discoverability backlog

Findings from driving the running app over CDP (`npm run tauri:debug` + Playwright
MCP). Each item records the observed behaviour, the cause in the code, and the
decided fix.

## Open

### 1. Recent-project cards show an empty box when thumbnails are missing

**Observed.** The welcome screen logs six `404` errors on every start:

```
http://cullant.localhost/recent-thumb/5/0?v=C:\Users\lalop\Pictures
http://cullant.localhost/recent-thumb/5/1   ... /5/2, /6/0, /6/1, /6/2
```

**Cause.** The import did not finish its thumbnail pass for those projects.
`respond_recent_thumb` (`src-tauri/src/protocol/mod.rs:294`) reads the project's
own `.cullant` cache and never generates a thumbnail that does not exist yet, so
the slot stays a `404` forever. The card then hides the broken image —
`onerror` sets `display: none` (`src/lib/components/ProjectGallery.svelte:129`).
The result is a blank 132×96 preview area with no explanation.

**Fix.** Replace the hidden image with a friendly coloured placeholder:

- Draw a placeholder in any slot whose thumbnail fails to load.
- Draw a placeholder in the spare slots when the project holds only one or two
  photos, so the fanned stack of three keeps its shape.
- Give each slot a different colour.

The card keeps the three `PREVIEW_SLOTS` and the hover fan-out, so the
placeholder must use the same 84×84 `.peek` geometry and rotation.

### 2. The delete control on a project card is too heavy

**Observed.** The accessibility snapshot shows `button "Delete Cullant data for
this project"` with no reference, because the button stays hidden until hover.

**Decided.** Hover-to-reveal is correct on desktop — keep it.

**Fix.** Make the control lighter (`ProjectGallery.svelte:315`):

- Reduce the 36×36 px box and its 18 px icon.
- Lighten the `rgba(18, 18, 22, 0.6)` background, which reads as a dark blob
  over the card.
- Keep the 44×44 px size under `@media (pointer: coarse)`. That is the touch
  target the rest of the app uses.

### 3. The commit dialog speaks touch language on desktop

**Observed.** The Pending tab shows: *"Tap a row to expand · hold it to run that
section on its own."* On desktop the user clicks and long-presses with a mouse.

**Fix.** Make the hint match the pointer type
(`src/lib/components/CommitDialog.svelte:653`). The app already branches on
`(pointer: coarse)` elsewhere.

### 4. Toolbar buttons signal "has work" by colour only

**Observed.** An active name filter turns the Sort & Filter button teal
(`rgb(34, 114, 104)`) through the `haswork` class. The accessible name stays
`"Sort & Filter Date"`. Nothing else marks the state.

**Cause.** `class:haswork` carries the whole signal, on three buttons:
Sort & Filter, View and Commit (`src/routes/+page.svelte:442`, `:466`, `:491`).

**Fix.** Add a non-colour cue — `aria-pressed`, a count, or a dot. This is
WCAG 1.4.1 (use of colour), and it also helps anybody who reads the toolbar at
a glance.

### 5. The grid does not tell assistive tech which photo is focused

**Observed.** The focused cell carries a `.focused` class and nothing more.
`document.activeElement` stays `BODY`, every cell is `tabindex="-1"`, and the
grid has no `aria-activedescendant` (`src/lib/components/VirtualGrid.svelte:871`).

**Not a bug in the focus model.** A global key dispatcher plus a virtual grid is
the right design here — real DOM focus cannot follow a virtualised list.

**Fix.** Give the grid `aria-activedescendant` pointing at the focused cell id.
The rating already reaches the accessibility tree correctly — a rated cell reads
`"★★★ img_0052.jpg"` — so only the focus position is missing.

### 6. A filter that matches nothing loses the focused photo

**Observed.** Focus sits on `img_0052.jpg`. Filter to zero matches, then clear
the filter: focus resets to `img_0000.jpg` instead of returning to
`img_0052.jpg`.

**Fix.** Keep the focused id while the filter empties the grid and restore it
when the id comes back into the filtered set.

### 7. Reopening a project can fail with a raw database error

**Observed.** Closing "Japon 2025" (577 files on an external `D:` drive) and
reopening it immediately failed:

> Couldn't open project — `database error: unable to open database file`

The retry a minute later succeeded. The failure left **no backend log line** —
it aborted before `opened project at`. The database file was intact and its
`-wal`/`-shm` companions were gone, so the previous session had closed cleanly.

**Likely cause.** An external drive that spun down, or a short-lived lock right
after close. Not reproducible on demand.

**Fix.** A photo culler reads projects from external drives and camera cards, so
a transient open failure is expected traffic, not an exception. Retry the open
before reporting, and log the failure. The current message also blames "the
project" for what is a storage hiccup.

### 8. Jumping ahead of the background pass leaves the viewer blank

**Observed.** With the preview pass still running and the drive saturated,
pressing `End` in the loupe to reach the last photo took:

| Request | Time |
|---|---|
| `thumb/556` | 12.9 s |
| `preview/556` | 14.7 s |

**Cause.** The loupe has no spinner by design. It softens the grid thumbnail
after 80 ms instead, and `ZoomImage.svelte:245` states the assumption: the grid
thumb is *"virtually always cached"*. That holds while the user moves inside the
generated range. It breaks the moment the user jumps past it — there is then no
thumbnail to soften and nothing to show.

The grid has the right control for this (`.preview-spin`,
`VirtualGrid.svelte:1020`), but its condition needs `loaded.has(item.id)`, so it
only appears once the **thumbnail** has painted. A cell still waiting for its
thumbnail shows no spinner either.

**Fix.** Show a progress indicator when neither artifact is cached. The data is
already there — `previewReady` says exactly which ids are ready.

### 9. The thumbnail counter reports a constant, not the work

**Observed.** On a *fresh* import of 577 files (529 grid items), the pill reads
`Thumbnails 15 / 60`, then `Thumbnails 60 / 60`, then the phase reports done and
the pill switches to `Previews X / 359`. Measured against the cache on disk:

| Time | Event | `_t.jpg` on disk |
|---|---|---|
| 21:16:08.426 | `scan:done` — 577 files, 38 ms | 0 |
| 21:16:12.039 | `metadata:done` — 407 files, 3.6 s, 11.06 GB | 0 |
| **21:16:25.266** | **`thumbs:done`** — pill reports 60 / 60 | **60** |
| 21:22:04 | preview pass complete | 364 |

The project ends with **412** thumbnail files. So the UI reports the thumbnail
work complete with **60 of 412 generated — 14.6 %**.

**Cause.** `total` is `LEAD_WINDOW` (`scan/ingest.rs:54`), a fixed 60. It is the
start-up window that stops the grid looking empty, not a measure of the work.
Every remaining thumbnail is generated by the fused pass, which reports itself
as "Previews" alone.

**Fix.** Report the fused pass for what it is, or fold the two pills into one
"Preparing photos" count over the real total. Reporting a constant as a
denominator is what makes a new project look almost finished.

### 12. Auto-rescan ignores `ingesting`

**Observed.** During the fresh import, a rescan fired at 21:21:08 while the
preview pass was still running (it finished at 21:21:59).

**Cause.** The guard is `if (storageOk && !catalog.scanning)`
(`src/routes/+page.svelte:330`). It tests `scanning` — the folder walk — but not
`ingesting`, which is the flag that reports the artifact passes as well. Its
sibling, the storage probe at `:296`, does test `ingesting`, and says why:
*"probing while the storage backend is already saturated is exactly when the
answer is worthless."* The rescan comment claims the same protection it does not
implement.

**Fix.** Guard on `catalog.ingesting`. The walk cost 38 ms here, but this is the
case the ingest is built around: an external drive, or Android SAF, where the
walk competes with the decode pass for the same saturated backend.

### 10. `close()` does not clear `previewReady`

**Cause.** `catalog.close()` clears `thumbLoaded` but not `previewReady`
(`src/lib/stores/catalog.svelte.ts:176`), while `open()` (`:112`) and
`adoptCurrent()` (`:77`) clear both.

**Impact is latent, not visible.** `open()` clears the set before any cell of
the next project renders. Worth fixing for symmetry: file ids are unique only
inside one project's database, which is the same invariant `close_project`
protects on the backend when it clears the memory cache.

### 11. `thumbs:done` fires more than once

**Observed.** Twice when reopening a fully generated project (894235 ms and
894243 ms). Three times at the end of the fresh import — 21:21:59.845, .954
and .957.

## Verified — the ingest resumes correctly

Driving a 577-file, 11 GB project through close/reopen twice:

| Phase | Cold open | Reopen, half done | Reopen, fully done |
|---|---|---|---|
| scan | 34 ms | 17.6 ms, `0 new` | 18.4 ms, `0 new` |
| metadata | 4.5 s · 407 files · 11.07 GB | 48.9 µs · 0 files | 106.6 µs · 0 files |
| thumbnails (lead) | 14.5 s · 60 | 8.4 s · 60 · 3.29 GB | nothing to do |
| previews | 234.8 s, interrupted | 109.1 s · **108**, not 359 | nothing to do |
| video posters | aborted by close | 57.6 s · 48 | nothing to do |

- **No work is repeated.** The second reopen ran the whole pipeline in **18 ms**
  with every phase reporting a total of 0, and the cache on disk did not change
  (412 thumbnails, 359 previews before and after).
- **The resume count is exact.** After the first reopen `previewReady` held 251
  ids and the disk held 251 preview files.
- **A cached preview opens instantly**: 28 ms in the loupe, no spinner — which
  is the correct behaviour, since nothing needs generating.
- **Closing does stop the background work.** `close_project` shuts the ThumbPool
  down and the drained phases end in ~120 µs.
- **`recent-thumb` serves what exists**: 8–22 ms for projects with a cache, 404
  only for projects that have none. Finding 1 stands as first diagnosed.
- **The fused pass does not regenerate the lead window.** On the fresh import the
  two caches grow apart at first — 60 thumbnails against 48 previews — and then
  together, holding a constant offset of ~13. The pass starts at the top of the
  grid, where the first 60 items already have their thumbnail, so it writes only
  their preview. Past the window it writes both. The gap is the proof that no
  thumbnail is made twice.

## Checked — no action needed

These looked suspicious from outside and turned out correct:

- **The touch action bar does not leak onto desktop.** `.touchbar-dock` reports
  `opacity: 1`, but its box is 0 px high at `y = 800`. It is collapsed, not
  visible.
- **"Retouch" and "Color grade" in that bar are task tags**, not editing
  features. Cullant still never modifies a photo.
- **Keyboard shortcuts are discoverable.** `?` opens the dialog, and
  Settings → "Keyboard shortcuts" reaches it by mouse. Every setting also has a
  "What does X do?" button.
- **The empty grid explains itself**: "No items match the current filters", with
  a button that opens Sort & Filter.
- **The commit dialog reports the right work** — rejects reach the delete queue
  and the XMP count follows the edits.

## Method

Attach any Chrome DevTools Protocol client to the running app. See
"Driving the UI from outside" in `CLAUDE.md`.

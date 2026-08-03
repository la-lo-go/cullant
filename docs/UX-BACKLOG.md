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

---
description: Strip non-essential comments file-by-file, keeping only short high-value ones (Clean Code style)
argument-hint: [path-or-glob-filter]
---

Run a repo-wide (or scoped) pass that removes low-value comments and keeps only the
kind a senior engineer would leave: short, and explaining something the code itself
cannot. This is the exact bar already defined in this repo's `CLAUDE.md` "Doing tasks"
section — apply it literally, don't invent a stricter or looser one.

## Scope

- If `$ARGUMENTS` is given, treat it as a pathspec/glob filter (e.g. `src-tauri/src/engine`,
  `src/lib/components`). Only touch files under that filter.
- Otherwise scope is the whole repo's own hand-written source: `src-tauri/src/**/*.rs`,
  `src/**/*.svelte` (this also covers the scoped `<style>` blocks inside them — there are no
  standalone `.css` files today, but if one shows up under `src/`, include it too),
  `src/**/*.ts`, and Kotlin under paths we actually own: `src-tauri/plugins/**/*.kt`,
  `src-tauri/plugins/**/*.kts`.
- Always exclude: `target/`, `node_modules/`, `.svelte-kit/`, `build/`, `dist/`,
  `src-tauri/src/db/migrations/**` (SQL, not comment cleanup territory), lock files,
  **`src-tauri/gen/**` entirely** (Tauri-scaffolded Android/Apple project — Gradle/Kotlin/XML
  boilerplate we don't hand-maintain; touching its comments is wasted effort and risks
  fighting the next `tauri android init` regen), and any other vendored/generated code.
- Get the exact file list with `git ls-files` filtered to those patterns, not a manual glob —
  it must match what's actually tracked.

## The keep/remove rubric

**Remove** a comment if it:
- Restates what the next line/block obviously does (`// increment counter` above `i += 1`).
- Describes *what* a function does when the name + types already say it.
- References the current task, a fix, an issue number, or a caller ("added for X flow",
  "fix for #123", "used by Y") — that belongs in a commit message, not the code.
- Is commented-out dead code.
- Is a purely decorative section banner (`// ---- Handlers ----`) with no information beyond
  visual grouping — lean toward removing unless the file is large enough that it's a real
  navigation aid (judgment call, err toward removing).
- Is a doc comment (`///`, `/** */`, JSDoc) that only restates the signature/params with no
  added constraint or caveat.
- Is a stale TODO/FIXME describing work that's already done or no longer relevant.

**Keep** a comment only if it explains a genuinely non-obvious WHY: a hidden constraint, a
subtle invariant, a workaround for a specific bug or platform quirk, or behavior that would
surprise a competent reader of this codebase. Always keep, regardless of length:
- License/copyright headers.
- `// SAFETY:` comments justifying an `unsafe` block.
- Lint-suppression comments with a reason (`#[allow(clippy::... )] // reason`,
  `// eslint-disable-next-line — reason`).
- A still-relevant TODO/FIXME that encodes real follow-up work.
- The handful of doc comments that carry an actual constraint (e.g. "must be called before
  X", "units are milliseconds", "returns None if the DB write thread is down") — trim these
  to the minimum needed to state the constraint, don't delete the constraint itself.

When in doubt on a specific comment, keep it — this is a comment-quality pass, not a
comment-quantity target. Never touch code logic or reformat surrounding code; the diff for
each file should be comment-only removals/trims.

## Execution

1. Build the file list per Scope above and report the count to the user before starting.
2. Process files in batches of ~8. For repo-wide runs, dispatch each batch to a forked
   subagent (`subagent_type: "fork"`) with this rubric restated concisely in the prompt (the
   fork inherits this conversation's context, including `CLAUDE.md`, but batch prompts should
   still name the exact file list for that batch and remind it to report per-file counts).
   For a small scoped run (a handful of files), just do it directly, no forking overhead.
3. Each batch/file: read the file, decide per the rubric, apply edits, and record for the
   final summary: file path, number of comments removed, and — for every comment *kept*
   despite looking removable at a glance — a one-line reason, so the user can audit judgment
   calls without re-reading every file.
4. After all batches land, run the repo's verification commands (comment-only changes should
   never break these, but confirm):
   - `cd src-tauri && cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test`
   - `npm run check`
5. Present the final summary: total files touched, total comments removed, and the list of
   judgment-call keeps with reasons. Do **not** commit — `CLAUDE.md` says commit only when
   asked. End by proposing a single atomic commit message for the whole pass (e.g.
   `chore: strip low-value comments across <scope>`) and wait for the user to confirm before
   committing.

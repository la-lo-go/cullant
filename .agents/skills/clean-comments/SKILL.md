---
name: clean-comments
description: Remove low-value comments from Cullant source files. Keep short comments that explain a constraint, invariant, workaround, or non-obvious reason. Use for comment cleanup across the repository or a subtree.
---

# Clean comments

Remove comments that repeat the code. Keep comments that prevent a competent reader from
making a wrong assumption.

Read `AGENTS.md` before you edit files. Read the relevant parts of `CLAUDE.md` when the scope
includes RAW, HEIF, Android, video, or platform decoders.

## Scope

Use the user-provided path or glob when one exists. Otherwise, inspect these tracked files:

- `src-tauri/src/**/*.rs`
- `src/**/*.svelte`
- `src/**/*.ts`
- `src-tauri/plugins/**/*.kt`
- `src-tauri/plugins/**/*.kts`

Build the list from `git ls-files`. Exclude generated, vendored, and build output. Always
exclude `target/`, `node_modules/`, `.svelte-kit/`, `build/`, `dist/`, `src-tauri/gen/**`,
`src-tauri/src/db/migrations/**`, lock files, and generated code.

## Remove

Remove a comment when it does one of these things:

- It restates the next statement or block.
- It describes behavior that the name and types already show.
- It records a task, fix, issue, or caller that belongs in version history.
- It contains dead code.
- It is a decorative section banner with no navigation value.
- It documents parameters or return values without adding a constraint.
- It is a stale `TODO` or `FIXME`.

## Keep

Keep a comment when it explains one of these things:

- A hidden constraint or required call order.
- A subtle invariant.
- A platform or library workaround.
- Behavior that would surprise a competent reader.
- A unit, sentinel value, failure mode, or compatibility contract that types do not show.
- The reason for a lint suppression or an `unsafe` block.
- A current `TODO` or `FIXME` that records real follow-up work.
- A license or copyright notice.

Shorten a useful comment when possible. Do not remove its constraint. Keep the comment when
the decision is uncertain.

## Execution

1. Report the exact file count before editing.
2. For a large run, split the files into batches of about eight files.
3. Give each isolated subagent the exact file list and the full keep/remove rules.
4. Tell each subagent to change comments only. It must not change logic or surrounding
   whitespace.
5. Record the number of removed comments for each file.
6. Record why each debatable comment stayed.
7. Review the combined diff for code or formatting changes before verification.

## Verification

Run checks without modifying unrelated formatting:

```text
cd src-tauri
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cd ..
npm run check
git diff --check
```

Report the touched-file count, removed-comment count, and debatable comments that stayed.
Do not commit unless the user asks. Suggest one atomic commit message for the completed pass.

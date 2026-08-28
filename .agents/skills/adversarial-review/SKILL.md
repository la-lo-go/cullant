---
name: adversarial-review
description: Review Cullant frontend and backend code with isolated reviewers, anchored findings, per-file scores, bounded fixes, and repository verification. Use for an adversarial review, deep code review, full source sweep, or review-and-fix pass.
---

# Adversarial review

Review the frontend and backend with isolated roles. Apply accepted fixes. Verify each pass.
Stop at a fixed limit.

Write prompts and reports in short, literal English. Use the `asd-ste100` skill when the text
is dense or ambiguous.

Read `AGENTS.md` first. Read the relevant architecture sections in `CLAUDE.md` before you
dispatch reviewers.

## Mode

Use `diff` when the user does not specify a mode. Review changes against the requested base.
Use `main` or its merge base when no base is given. If `main` does not exist, compare the
worktree with `HEAD`. Include untracked source files.

Use `sweep` to review all source files. Split a sweep into the module batches in Scope.

Apply an optional path or glob after the mode. Review only matching files.

## Scope

Build source lists from `git ls-files` and relevant untracked files from `git status`.

Backend batches:

- `src-tauri/src/commands/`
- `src-tauri/src/db/`
- `src-tauri/src/scan/`
- `src-tauri/src/decode/`
- `src-tauri/src/thumbs/`
- `src-tauri/src/protocol/`
- `src-tauri/src/engine/`

Frontend batches:

- `src/lib/stores/`
- `src/lib/keyboard/`
- `src/lib/api.ts`
- `src/lib/components/`
- `src/routes/`

Exclude `target/`, `node_modules/`, `.svelte-kit/`, `build/`, `dist/`, `src-tauri/gen/**`,
`src-tauri/src/db/migrations/**`, lock files, generated code, and vendored code.

## Roles

Use separate subagents for each role. Do not let a reviewer see another reviewer's output.
Do not give a reviewer the current conversation. A reviewer must not edit files.

- Dispatch two frontend reviewers for each frontend batch.
- Dispatch two backend reviewers for each backend batch.
- Use different available models when the runtime supports model selection. Otherwise, use
  independent contexts and opposite review stances.
- Dispatch one arbiter after all raw findings arrive.
- Dispatch one applier only after the arbiter accepts findings.

Give each reviewer only the target files, this rubric, and the relevant repository rules.
Give the arbiter only the raw findings and scores. Give the applier only accepted findings
and target files.

## Review axes

Review each file on these axes:

- Naming: names must carry meaning that code can express.
- Reuse: report real duplication, a missing abstraction, or an abstraction that is too broad.
- Readability: report deep nesting, high complexity, or mixed responsibilities.
- Architecture: enforce repository boundaries from `AGENTS.md` and `CLAUDE.md`.
- Stability: report panics, unsafe hot-path assumptions, swallowed errors, races, and concrete
  edge cases.
- Performance: report measurable hot-path allocation, repeated database work, UI-thread work,
  or unnecessary Svelte updates.
- Comments: keep only a non-obvious reason, invariant, constraint, or workaround.

Each finding must use this shape:

```text
{axis, severity(must|should|nice), file:line, anchor(rule|failure-scenario), one-sentence fix}
```

The anchor must name a repository rule or a concrete failure scenario. Drop taste-only
findings. Send subjective findings to the backlog.

## Per-file score

Each reviewer scores every file from 1 to 100.

- 1-20: the file does not compile or is structurally broken. A non-compiling file has a hard
  cap of 20.
- 21-40: the file compiles but has severe debt or a probable bug.
- 41-60: the file works but has moderate debt and several anchored findings.
- 61-80: the file is solid and has minor anchored findings.
- 81-95: the file is polished and has only subjective or small findings.
- 96-100: theoretical quality. Do not target this band.

Apply these bias controls:

1. A file that passes its required checks has a score floor of 40.
2. Review each file from an adversarial stance and an advocate stance.
3. Each reduction from 100 needs an anchored finding.
4. Combine the two reviewer scores with the median.
5. Apply a fix only when its estimated score gain is at least three points.

## Bounded loop

Use these defaults:

```text
TARGET = 85
MIN_DELTA = 3
MAX_PASSES = 3
FILE_CAP = 40
```

For each pass:

1. Dispatch frontend and backend reviewers in parallel.
2. Collect raw findings and per-file scores.
3. Remove files with a median score at or above `TARGET` from the pool.
4. Ask the arbiter to reject findings without an anchor.
5. Ask the arbiter to move `nice` findings to the backlog.
6. Ask the arbiter to reject fingerprints that the ledger already marks as applied or
   rejected.
7. Ask the arbiter to deduplicate and order the remaining findings by severity.
8. Ask the applier to make the smallest fix for each accepted finding with an estimated gain
   of at least `MIN_DELTA`.
9. Verify the repository.
10. Revert a fix that breaks verification. Mark its fingerprint as rejected. Do not retry it.
11. Re-score touched files. Record each score, fingerprint, changed region, and result.

Stop when one condition is true:

- The review pool is empty.
- The aggregate score gain for a pass is less than `MIN_DELTA`.
- The review reaches `MAX_PASSES`.
- The review would touch more than `FILE_CAP` files.

Mark a file that remains below `TARGET` as requiring human attention. Do not restart it after
the limit.

## Verification

Run the checks that match the touched areas. Run all checks after a mixed or repository-wide
pass.

```text
cd src-tauri
cargo fmt -- --check
cargo clippy --all-targets -- -D warnings
cargo test
cd ..
npm run check
git diff --check
```

Run the app when a fix changes runtime behavior.

## Artifacts and report

Write review state to `.cullant-review/`:

- `ledger.json`: applied and rejected fingerprints, changed regions, and hashes.
- `report.md`: pass findings, fixes, reversions, verification, and score trajectories.
- `BACKLOG.md`: subjective and `nice` findings for human review.

Show a score trajectory for each reviewed file. Explain each stop condition. List files that
need human attention.

Do not commit unless the user asks. Propose atomic commit messages grouped by axis or module.

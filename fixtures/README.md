# Test corpus — real camera files

Files a camera actually wrote, for the tests that synthetic images cannot serve.

`examples/gen_testdata.rs` makes JPEGs, and they are the right tool for anything
about counts, ordering or state. They are the wrong tool for the decoder: a
synthetic JPEG has no maker notes, no embedded preview at a vendor's private
offset, no `irot` disagreeing with EXIF, and no sibling that belongs to the same
shot. Every one of those has already produced a bug here.

## Getting the corpus

```sh
npm run fixtures              # grouping pair + one file per brand, ~90 MB
npm run fixtures -- --all     # every format Cullant claims to read, ~350 MB
npm run fixtures -- --max-mb 8
npm run fixtures -- --tier core
```

Downloads land in `media/`, which git ignores. Re-running skips what is already
there and correct.

## Why the media is not in git

A corpus covering every RAW format is a few hundred megabytes, and git keeps
every byte of it forever — a file committed once cannot be removed without
rewriting history. The repo is 16 MB today.

So `manifest.json` is tracked and the bytes are not. It carries a SHA-256 per
file, which the fetcher verifies **before writing**: a truncated or substituted
download never reaches the corpus, where it would look like a decoder bug months
later.

If you would rather have the files in the repo, delete `media/` from
`.gitignore` — but decide that before the first push, not after.

## Licensing

Everything here is **CC0** (public domain), from [raw.pixls.us][pixls], which
requires CC0 for everything it hosts. Cullant is GPL-3.0-or-later and CC0 sits
under that without a compliance note.

**Adding a file: the licence is the entry requirement, not an afterthought.**
Add it to `manifest.json` with its `url`, `sha256` and a `license` field naming
the actual licence, and only add what you can point at a licence for. "Found it
online" is not one. Your own photos are fine — you hold the copyright — but say
so in the entry.

[pixls]: https://raw.pixls.us/

## What is in it

| Tier | What it is |
|---|---|
| `group` | An Olympus E-M1 II `ORF`+`ORI` pair: **one shot, two files**. The only fixture that exercises N-ary grouping on files a camera wrote. |
| `core` | The smallest CC0 sample of each format `decode::RAW_EXTS` claims. |
| `brand` | One per maker, where another maker already covers the format — a Leica DNG is not a Canon DNG. |

## Gaps, and they matter

- **No HEIF.** pixls.us hosts RAW only, and the HEIF samples elsewhere have
  licences I could not establish. The ladder in `decode/heif.rs` is therefore
  tested against a synthetic container and one hand-checked file. A `.HEIC`
  straight off a phone is the single most useful thing to add here — it also
  settles whether `kamadak-exif` refusing an EXIF block over 64 KB (which an
  iPhone's maker note can exceed) is a real problem.
- **No video.** `decode/video.rs` has no corpus at all.
- **No RAW+JPEG pair.** The `ORF`+`ORI` pair covers grouping, but not the
  RAW+JPEG mirror mode that most cameras actually produce.

## Running the tests against it

```sh
cd src-tauri && cargo test the_corpus -- --nocapture
```

It walks `media/`, runs the real ingest over every file, and reports what it
read from each. It skips when the corpus is absent, so it never fails CI on a
machine that has not fetched it.

`KNOWN_UNRENDERABLE` in that test lists the files this build cannot render. It
is an allowlist so that anything *new* breaking fails the test — and it fails
just as loudly when a listed file starts working, so the list cannot rot.

## What the first eight files found

Every one was dated and named from real EXIF. Three would not render:

- **Olympus `.ORF` and `.ORI`** — the file holds a ~1 MB JPEG preview and
  nothing here reaches it. Olympus stores the offset in the MakerNote, and
  `decode/raw.rs` walks IFD0, the chained IFDs and the SubIFDs but not that.
  **Every Olympus and OM System shot therefore draws a blank cell**, including
  the Live ND pair this corpus exists to exercise. The clearest real bug the
  corpus has produced.
- **Panasonic `.RAW` (DMC-FZ8)** — holds no JPEG at all, so only a demosaic
  would render it. Correct to fail; arguably wrong to tombstone.
- **Sigma `.DNG` (fp)** — holds an 8 KB thumbnail and nothing larger.

It also found a Sigma fp writing Make `SIGMA` and Model `SIGMA fp`, which the
camera filter listed as "SIGMA SIGMA fp" beside the same body's other spelling.
Fixed in `decode::camera_name`.

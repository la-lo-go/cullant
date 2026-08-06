# Test corpus: real camera files

This corpus holds files that cameras wrote. The tests that synthetic images
cannot serve use it.

`examples/gen_testdata.rs` makes JPEGs. Those JPEGs are correct for tests about
counts, order or state. They are wrong for tests about the decoder. A synthetic
JPEG has none of the four things that break real decoders:

- maker notes
- an embedded preview at a vendor's private offset
- an `irot` property that disagrees with the EXIF orientation
- a sibling file from the same shot

Each one of those four has already caused a defect in this repository.

## How to get the corpus

```sh
npm run fixtures              # the grouping pair, plus one file per brand: ~90 MB
npm run fixtures -- --all     # every format Cullant claims to read: ~350 MB
npm run fixtures -- --max-mb 8
npm run fixtures -- --tier core
```

The files arrive in `media/`, which git ignores. Run the command again at any
time. It skips each file that is already present and correct.

## Why git does not store the media

A corpus that covers every RAW format holds a few hundred megabytes. Git keeps
every byte forever: once a commit contains a file, only a rewrite of the history
removes it. The repository holds 16 MB today.

Git therefore tracks `manifest.json` and not the bytes. The manifest holds one
SHA-256 per file. The fetcher checks that hash twice — once before it writes the
file, and once after. A truncated or substituted download never enters the
corpus, where it would look like a decoder defect months later.

To store the files in the repository instead, delete `media/` from `.gitignore`.
Make that decision before the first push, not after it.

## Licences

Every file here is **CC0**, which means public domain. They come from
[raw.pixls.us][pixls], which accepts only CC0 files. Cullant is
GPL-3.0-or-later, and CC0 imposes no compliance note on that.

**A licence is the condition of entry.** To add a file:

1. Find the licence. "I found it online" is not a licence.
2. Add an entry to `manifest.json` with the `url`, the `sha256` and a `license`
   field that names the licence.

Your own photographs are acceptable, because you hold the copyright. Write that
in the entry.

[pixls]: https://raw.pixls.us/

## What the corpus contains

| Tier | Contents |
|---|---|
| `group` | One Olympus E-M1 II `ORF`+`ORI` pair: **one shot, two files**. This is the only fixture that tests N-ary grouping on files a camera wrote. |
| `core` | The smallest CC0 sample of each format that `decode::RAW_EXTS` lists. |
| `brand` | One file per maker, where a different maker already covers that format. A Leica DNG is not a Canon DNG. |

## Gaps

- **The corpus holds no HEIF.** raw.pixls.us hosts RAW files only. The licences
  of HEIF samples on other sites are unclear. Two things therefore test the
  ladder in `decode/heif.rs`: a synthetic container, and one file checked by
  hand. A `.HEIC` from a phone is the most useful file to add. It also answers
  one open question: `kamadak-exif` refuses an EXIF block larger than 64 KB, and
  an iPhone maker note can exceed that size.
- **The corpus holds no video.** No real file backs `decode/video.rs`.
- **The corpus holds no RAW+JPEG pair.** The `ORF`+`ORI` pair tests grouping. It
  does not test the RAW+JPEG mirror mode that most cameras produce.

## How to run the tests against it

```sh
cd src-tauri && cargo test the_corpus -- --nocapture
```

The test copies every file from `media/` into a temporary project, runs the real
ingest, and prints what the app read from each file. It skips itself when the
corpus is absent, so it never fails on a machine that did not fetch it.

`KNOWN_UNRENDERABLE` inside that test lists the files this build cannot render.
It fails the test in both directions:

- A file that is not on the list, and stops rendering, fails the test.
- A file that is on the list, and starts rendering, also fails the test.

The second rule keeps the list accurate.

## What the first eight files found

The app dated and named all eight from real EXIF. Three did not render:

- **Olympus `.ORF` and `.ORI`.** The file holds a JPEG preview of about 1 MB
  that no code here reached. Olympus stores the offset in the MakerNote, and
  `decode/raw.rs` walked IFD0, the chained IFDs and the SubIFDs only. **Every
  Olympus and OM System shot showed an empty cell**, including the Live ND pair
  that this corpus exists to test. This is the clearest defect the corpus has
  found. **Fixed:** `raw.rs` now reads the MakerNote. See
  [formats.md](../docs/formats.md#where-a-maker-hides-the-preview).
- **Panasonic `.RAW` from a DMC-FZ8.** The file holds no JPEG at all. Only a
  demosaic can render it. The failure is correct. The tombstone is arguable.
- **Sigma `.DNG` from an fp.** The file holds an 8 KB thumbnail and nothing
  larger.

The corpus also found a Sigma fp that writes `SIGMA` as the Make and `SIGMA fp`
as the Model. The camera filter listed that body as "SIGMA SIGMA fp", beside the
same body under its other spelling. `decode::camera_name` now corrects it.

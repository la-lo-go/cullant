# Formats

What Cullant catalogues, what it renders, and where the two differ. The
extension lists live in `src-tauri/src/decode/mod.rs`; this explains them.

**Catalogued** means the file gets a database row: it is counted, grouped with
its siblings, filtered, rated, deleted, moved and XMP-exported like any photo.
**Rendered** means a thumbnail and a loupe preview exist.

The two are deliberately separate. An extension Cullant does not recognise is
skipped entirely — which is how a rejected shot used to leave its `.HIF` behind
as an orphan the user never knew existed.

## RAW

Every extension below is catalogued and grouped. Rendering comes from the
camera's own embedded JPEG preview (the Photo Mechanic trick), extracted by
`decode/raw.rs` where it can, and by `rawler` otherwise. No demosaic.

| Maker | Extensions |
|---|---|
| Canon | `cr3` `cr2` `crw` |
| Nikon | `nef` `nrw` |
| Sony | `arw` `sr2` `srf` |
| Fujifilm | `raf` |
| Panasonic | `rw2` `raw` |
| OM System / Olympus | `orf` `ors` `ori` |
| Leica, Sigma, Pentax, phones | `dng` |
| Pentax / Ricoh | `pef` |
| Samsung | `srw` |
| Sigma | `x3f` |
| Hasselblad | `3fr` `fff` |
| Phase One | `iiq` |
| Epson, Mamiya, Kodak, Leaf | `erf` `mef` `mos` `kdc` `dcr` |

Compression variants (Canon C-RAW, Nikon HE★, Sony lossless L/M/S, Phase One
IIQ S v2 …) are mostly irrelevant: the preview is extracted without touching the
RAW payload. They only matter when no preview is found and `rawler` has to
decode the sensor data itself.

### Known gaps

Found by the real-file corpus (see [testing.md](testing.md)), pinned in that
test's `KNOWN_UNRENDERABLE` so they cannot be forgotten:

- **Olympus / OM System `orf` and `ori` do not render.** The file holds a ~1 MB
  JPEG preview and nothing here reaches it: Olympus keeps the offset in the
  MakerNote, and `raw.rs` walks IFD0, the chained IFDs and the SubIFDs only.
  Metadata is read fine, so those photos sort and filter correctly — they just
  draw a blank cell.
- **Panasonic `.RAW` (older bodies)** can hold no JPEG at all. Only a demosaic
  would render it.
- **Some `.DNG`** carry a thumbnail too small to use — a Sigma fp writes 8 KB
  and nothing larger.

## HEIF

`heic` `heif` `hif` `hsp`

**Metadata always.** `kamadak-exif` parses the ISO-BMFF item structure, so a
HEIF-only library — every stock iPhone — sorts by real capture time and fills
every filter facet, whatever the pixels do. Dimensions come from
`PixelXDimension`/`PixelYDimension`, because no header probe here can open the
container.

**Pixels depend on the machine.** Cullant has no HEVC decoder and will not grow
one: everything that decodes HEVC is a C dependency, which would be the first in
this build and would have to cross-compile for four Android ABIs. So
`decode/heif.rs` borrows one, trying in order:

| Rung | Where | Needs |
|---|---|---|
| Store's own | Android SAF | `ImageDecoder`, API 28+ (`minSdk` is 24, so it is asked at runtime) |
| In process | Windows | WIC + Microsoft's HEIF/HEVC extensions, which many machines lack |
| Subprocess | any desktop | `ffmpeg` **7.0+** — 6.x decodes HEVC but cannot demux a still |
| — | iOS | not built yet; drops in beside WIC |

A rung that has the codec and still refuses a file falls through to the next
one. Decoders disagree about what a HEIF is: WIC wants the item-based structure
a camera writes and rejects a lone HEVC frame in an MP4 that ffmpeg reads
happily.

Where no rung answers, the cell stays empty and **no tombstone is written** — a
tombstone is keyed on the file's mtime, and installing a decoder changes no
file's mtime, so one written here would leave that photo blank forever after the
upgrade.

## Images

`jpg` `jpeg` `png` `tif` `tiff` `webp` `bmp` `gif`

Decoded in process. JPEG takes an IDCT-scaled path (`decode/jpeg.rs`) that
decodes a quarter or a sixteenth of the pixels when the target is small enough.

This list is also what governs the **sibling borrow**: a RAW's thumbnail is
rendered from its paired JPEG, because that is the same frame for a fraction of
the bytes. HEIF is deliberately excluded even where it decodes, since borrowing
it would trade the RAW's own embedded JPEG for a subprocess or a JNI hop.

## Video

`mp4` `mov` `m4v`

Poster frames are borrowed from the platform the same way: `ffmpeg` on a real
filesystem, `MediaMetadataRetriever` through the SAF plugin on Android. ffmpeg
is an optional runtime dependency — absent, video thumbnailing is skipped
gracefully, and installing it later retries on the next scan.

Playback is a separate three-rung ladder (`VideoPlayer.svelte`): native
`<video>`, then an in-app transmux with mediabunny, then an external app.

## Sidecars

`xmp`, read on scan and written on commit. Rating, flag, label and orientation.
Both halves of a group share `IMG.xmp` while they agree; a group that diverges
exports the primary to `IMG.xmp` and the other half to `IMG.JPG.xmp`, the form
Bridge and exiftool use.

## Grouping

Files sharing a directory and basename are one shot, however many there are.
`IMG_0421.CR3` + `IMG_0421.JPG` is the common case; `ORF`+`ORI`+`JPG` (OM System
Live ND) and RAW+`HIF` are not.

The group's **primary** — the file whose frame stands for the shot, and the only
one whose thumbnail is pregenerated — comes from `decode::primary_rank`:

1. RAW
2. an in-process decodable image
3. a companion RAW (`.ORI`)
4. HEIF
5. anything else

A companion `.ORI` outranks a HEIF even though the HEIF is the better picture,
because the `.ORI` decodes unconditionally and a HEIF only where the ladder has
a rung. The ranking is static on purpose: it is persisted in
`groups.primary_file_id`, which travels with the project folder, so a
runtime-dependent rank would move the shot's cell on every other machine.

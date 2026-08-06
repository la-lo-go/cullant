# Formats

This document tells you which files Cullant catalogues, which files it renders,
and where the two differ. The extension lists are in
`src-tauri/src/decode/mod.rs`.

**Catalogued** means the file gets a database row. The app then treats it
exactly like any other photo. It:

- counts it
- groups it with its siblings
- filters, rates and labels it
- deletes and moves it
- exports it to XMP

**Rendered** means a thumbnail and a loupe preview exist for it.

The two are separate on purpose. Cullant skips any extension it does not know.
Before N-ary grouping, that behaviour left a `.HIF` on disk after the user
rejected the shot it belonged to.

## RAW

Cullant catalogues and groups every extension below.

To render one, it extracts the JPEG preview that the camera embedded in the
file. This is the Photo Mechanic method. `decode/raw.rs` does the extraction
where it can, and `rawler` does it otherwise. Neither one demosaics the sensor
data.

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

Compression variants usually do not matter. Canon C-RAW, Nikon HE★, Sony
lossless L/M/S and Phase One IIQ S v2 all store a preview. The extraction never
touches the compressed sensor data.

A variant matters only when the file holds no preview. `rawler` must then decode
the sensor data itself.

### Known gaps

The real-file corpus found these. See [testing.md](testing.md). The corpus test
pins each one in its `KNOWN_UNRENDERABLE` list, so nobody can forget them.

- **Olympus and OM System `orf` and `ori` do not render.** The file holds a JPEG
  preview of about 1 MB, and no code here reaches it. Olympus stores the offset
  in the MakerNote. `raw.rs` walks IFD0, the chained IFDs and the SubIFDs only.
  The app still reads the metadata correctly, so these photos sort and filter
  correctly. They show an empty cell.
- **Older Panasonic `.RAW` files hold no JPEG.** Only a demosaic can render one.
- **Some `.DNG` files hold a preview that is too small.** A Sigma fp writes 8 KB
  and nothing larger.

## HEIF

`heic` `heif` `hif` `hsp`

**Cullant always reads the metadata.** `kamadak-exif` parses the ISO-BMFF item
structure. A library that holds only HEIF files — every unmodified iPhone —
therefore sorts by real capture time and fills every filter facet. The
dimensions come from the `PixelXDimension` and `PixelYDimension` tags, because
no header probe here can open the container.

**The pixels depend on the machine.** Cullant has no HEVC decoder and will not
get one. Every library that decodes HEVC is a C dependency. Such a dependency
would be the first in this build, and it would have to cross-compile for four
Android ABIs.

`decode/heif.rs` borrows a decoder instead. It tries each rung in this order:

| Rung | Platform | Requirement |
|---|---|---|
| The store's own decoder | Android SAF | `ImageDecoder`, API 28 or later. `minSdk` is 24, so the app asks at runtime. |
| An in-process decoder | Windows | WIC, plus Microsoft's HEIF and HEVC extensions. Many machines do not have them. |
| A subprocess | Any desktop | `ffmpeg` 7.0 or later. Version 6 decodes HEVC but cannot demux a still image. |
| — | iOS | Not built yet. It fits beside the WIC rung. |

A rung that has the codec, and still refuses the file, hands the file to the next
rung. Decoders disagree about what a HEIF is. WIC expects the item-based
structure that a camera writes, and refuses a single HEVC frame inside an MP4.
`ffmpeg` reads that same file without complaint.

When no rung answers, the cell stays empty and the app writes **no tombstone**.
The app keys a tombstone on the file's mtime, and installing a decoder changes no
file's mtime. A tombstone written here would keep that photo blank after the
upgrade.

## Images

`jpg` `jpeg` `png` `tif` `tiff` `webp` `bmp` `gif`

Cullant decodes these in its own process. For JPEG it uses an IDCT-scaled path in
`decode/jpeg.rs`, which decodes a quarter or a sixteenth of the pixels when the
target size allows it.

This list also controls the **sibling borrow**. The app renders a RAW's thumbnail
from the JPEG beside it, because that JPEG holds the same frame in far fewer
bytes. HEIF stays off this list, even where a rung decodes it. Borrowing a HEIF
would replace the RAW's own embedded JPEG with a subprocess or a JNI call.

## Video

`mp4` `mov` `m4v`

Cullant borrows a poster-frame extractor the same way it borrows a HEIF decoder.
On a real filesystem it calls `ffmpeg`. On Android it calls
`MediaMetadataRetriever` through the SAF plugin.

`ffmpeg` is an optional runtime dependency. When it is absent, the app skips
video thumbnails and writes no tombstone. Installing it later makes the next scan
try again.

Playback uses a separate ladder of three rungs, in `VideoPlayer.svelte`: the
native `<video>` element, then an in-app transmux with mediabunny, then an
external application.

## Sidecars

Cullant reads `xmp` on each scan and writes it on each commit. A sidecar carries
the rating, the flag, the label and the orientation.

Both members of a group share one `IMG.xmp` file while their states agree. When
the states differ, the app writes the primary to `IMG.xmp` and the other member
to `IMG.JPG.xmp`. Adobe Bridge and exiftool both use that second form.

## Grouping

Files that share a directory and a basename are one shot, whatever their number.
`IMG_0421.CR3` plus `IMG_0421.JPG` is the common case. `ORF` plus `ORI` plus
`JPG`, which OM System writes in Live ND, is not. Nor is RAW plus `HIF`.

Each group has one **primary**. The primary supplies the frame that represents
the shot, and it is the only member whose thumbnail the app pregenerates.
`decode::primary_rank` chooses it in this order:

1. a RAW
2. an image the app decodes in process
3. a companion RAW, such as `.ORI`
4. a HEIF
5. anything else

A companion `.ORI` outranks a HEIF, although the HEIF holds the better picture.
The reason is certainty: the `.ORI` decodes on every machine, and a HEIF decodes
only where the ladder has a rung.

This ranking never changes at runtime. The database stores the result in
`groups.primary_file_id`, and that file travels with the project folder. A
ranking that depended on the machine would move the shot's cell each time the
user opened the project somewhere else.

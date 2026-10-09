# DGN format notes (cadkit-dgn)

What `crates/cadkit-dgn` reads, the byte layouts it relies on, and the evidence behind
each one. Offsets are hexadecimal, little-endian unless stated, relative to the start of
the structure named in the table. "Confirmed" means reproduced on the samples listed in
[Test corpus](#test-corpus) and, where possible, checked against an independent oracle;
"inferred" means seen in one sample only; "unverified" means ported or assumed without a
sample that exercises it.

## Status

| Area | V8 | V7 |
|---|---|---|
| Container | CFB (own reader: header, DIFAT, FAT, mini stream), zlib pages | flat record stream |
| Models | every `Dgn-Md/#NNNNNN` storage; names from the model index; 2D/3D | one implicit model |
| Units | model header UOR/master, global origin, master/sub unit definitions | TCB (sub/master, UOR/sub, labels, VAX origin) |
| Levels → layers | level table (names, ids); flags kept raw | used levels as `Level N` |
| Colors | default palette + type-5 color table; extended (>255) colors unresolved | default palette + type-5 color table |
| Elements | 2, 3, 4, 6, 7, 11, 12, 14, 15, 16, 17, 18/19 (group), 21, 22, 27, 34, 35, 37, 94 | same families except raster |
| Tags | tag sets (type 39) + tag elements attached to their target | decoded, **not attached** (no sample) |
| Unmodelled | `Unknown { type_name: "dgn.type_N" }` + props, never dropped | same |

## Sources

Clean-room: no ODA/Bentley SDK material, no GPL code. Sources used:

| Source | License | Used for |
|---|---|---|
| `[MS-CFB]` Compound File Binary format (public Microsoft open specification) | open spec | CFB header, FAT/DIFAT, directory, mini stream |
| `[MS-OLEPS]` property sets (public Microsoft open specification) | open spec | `\u0005SummaryInformation` (application name, code page) |
| ezdgn <https://github.com/monozukuri-ai/ezdgn> `docs/v8/FORMAT_NOTES.md`, `crates/ezdgn-core/src/v8/*.rs`, `src/entities.rs` | MIT | V8 page/record framing, model index and model header search (`find_model_header`), common header, line/vertex/ellipse/arc/text/text-node offsets, string-linkage framing; V7 description-length rule (`container_descriptor`), V7 shared cell offsets |
| GDAL dgnlib `ogr/ogrsf_frmts/dgn/{dgnread.cpp,dgnhelp.cpp,dgnopen.cpp,dgnlib.h}` <https://github.com/OSGeo/gdal> | MIT/X | V7 record header, display header, TCB, color table, every V7 element layout, tag set / tag value layout, default 256-color palette (`abyDefaultPCT`, ported to `src/palette.rs`), RAD-50, linkage framing, quaternion convention |
| GDAL `autotest/ogr/data/dgnv8/test_dgnv8_ref.csv` (ODA-driver output) | MIT/X | black-box oracle only (no layout knowledge taken from it) |
| Own hex analysis of the samples below | — | everything marked "inferred" and all V8 tag/table/raster layouts |

Ported code is attributed at the definition (`palette.rs`, `native/v7.rs`
`description_end` / `shared_cell`, `native/v8.rs` `parse_model_header`).

## Test corpus

Not committed; fetched into `corpus/public/` (record these in `scripts/fetch-corpus.py`).

| File | Source / license | Bytes | SHA-256 |
|---|---|---:|---|
| `gdal/test_dgnv8.dgn` + `test_dgnv8_ref.csv` | GDAL autotest (MIT/X) | 27,648 | `8f32f87c…1d1df0` |
| `gdal/smalltest.dgn` (V7 2D) | GDAL autotest (MIT/X) | 10,752 | `9d9faddb…4a284` |
| `gdal/knot_oob.dgn` (V7, malformed) | GDAL autotest (MIT/X) | 1,578 | `bd09d118…da4e` |
| `gdal/seed_2d.dgn`, `gdal/seed_3d.dgn` (V7 seeds) | GDAL `ogr/ogrsf_frmts/dgn/data` (MIT/X) | 9,216 / 2,048 | `dd8465f1…b527c` / `97c2f00e…b56896` |
| `safe/TreeTextNodeLabelsWithTags.dgn` (V8 2D, tags) | Safe Software support article "Reading MicroStation DGN Tags with FME", attachment <https://support.safe.com/hc/article_attachments/36260727163917> (public download, cited by ezdgn PROVENANCE) | 847,360 | `8439e6d3a18227122c10c5367799561ebe94737d9d27932f4eb89d743b108270` |
| `safe/Water_distribution_mains.dgn` (V8 2D) | Safe Software support article "Handling MicroStation DGN Item Types with FME", attachment <https://support.safe.com/hc/article_attachments/36769975369869> | 3,840,000 | `f34bbb1b408b583d9cfc9d280e4d5782559cfa824b4b51210a03cf5c88235b64` |
| private V8i 3D sample (`corpus/private`) | client drawing, local only | — | structural facts only |

GDAL seeds: `https://raw.githubusercontent.com/OSGeo/gdal/master/ogr/ogrsf_frmts/dgn/data/seed_{2d,3d}.dgn`.
The Safe files carry no explicit license; they are used locally as interoperability
inputs and never redistributed.

---

## V8 container

### Compound file

Standard `[MS-CFB]`: signature `D0 CF 11 E0 A1 B1 1A E1`, 512-byte header, sector shift
`0x1E` (9 = 512-byte sectors, 12 = 4096), mini sector shift 6, FAT sector count `0x2C`,
first directory sector `0x30`, mini stream cutoff `0x38` (4096), first mini FAT sector
`0x3C`, first DIFAT sector `0x44`, DIFAT count `0x48`, 109 header DIFAT entries at `0x4C`.
Directory entries are 128 bytes (name UTF-16 `0x00`, name length `0x40`, type `0x42`,
left/right/child `0x44`/`0x48`/`0x4C`, start sector `0x74`, size `0x78`; the high size
dword is ignored for version 3). Streams below the cutoff live in the mini stream (the
root entry's chain). All observed DGN files are version 3.

Short final sector: `[MS-CFB]` files are whole sectors, but 2 of 112 private V8 files
(written by the same application version as the other 110) end inside their last sector,
109 and 214 bytes after its start. That sector holds the tail of the last graphic page
(`Dgn^G/$4`), and the stream's directory size ends within the bytes present. The reader
accepts a final sector cut short when it covers the bytes the sized stream still needs
(zlib inflation of the page then completes without a problem); a tail shorter than that
is still truncation. Before this rule the whole page was skipped with
`dgn.stream_unreadable`, losing 751 and 784 elements (about a quarter of each file).

Safety: every sector id is bounds-checked, chains are cut after `file_len / sector_size`
sectors (cycles), sibling trees are walked iteratively with a visited set, storage nesting
is capped at 64. `sniff` parses only header + FAT + directory and requires a root entry
named `Dgn~H`; other OLE files (`.doc`, `.xls`, `.msg`) are rejected.

### Streams (confirmed on all four V8 files)

| Path | Content | zlib at |
|---|---|---|
| `Dgn~H` | file header (1576 inflated bytes; not decoded) | `0x14` |
| `Dgn~S` | 208 bytes, mostly `ff` (not decoded) | — |
| `Dgn~Mf` | 276 inflated bytes (not decoded) | `0x00` |
| `Dgn^Ix/Dgn~Mix` | model index | `0x00` |
| `Dgn-Md/#NNNNNN/Dgn~Mh` | model header (one type-66 element) | `0x00` |
| `Dgn-Md/#NNNNNN/Dgn^G/$n` | graphic element pages | page |
| `Dgn-Md/#NNNNNN/Dgn^C/$n` | control element pages | page |
| `Dgn-Md/#NNNNNN/Dgn^GA/$n`, `Dgn^CA/$n` | auxiliary (XAttribute) pages; `^AH` = 4-byte counter | page |
| `Dgn^Nm/$n`, `Dgn^NmA/$n` | file-level ("non-model") elements: tables, tag sets, shared cell definitions | page |
| `\u0005SummaryInformation` | OLE property set: PID 1 code page (1200 observed), PID `0x12` application (VT_LPWSTR), PID `0x80000000` locale | — |
| `.Embedded/.Index`, `Oda~SH`, `DgnTk`, other `\u0005…` | not decoded | — |

`$n` page numbers sort numerically (`$10` after `$9`). `#NNNNNN` is the decimal storage
index. Pages of 16 bytes have no payload (record count 0).

### Page stream

| Offset | Type | Meaning |
|---|---|---|
| `0x00` | u32 | record count |
| `0x04` | u32 | page format version (2 or 3 observed) |
| `0x08` | u32 | page number (0 in some writers) |
| `0x0C` | u32 | population (0 in some writers) |
| `0x10` | zlib | records |

Records follow back to back: `u32` prefix (always 0 observed), then the element. Element
length is `2 * words` (`u32` at element `+4`). On every page of every sample the walk ends
exactly at the payload end and the count equals `record_count` (58 graphic + 25 named in
test_dgnv8; 33,335 graphic in TreeText; 66,053 in Water; 2,032 graphic / 830 control /
818 named in the private sample). The reader stops at the first record that does not
fit, keeps what it has and warns (`dgn.page_records`, `dgn.page_count`).

### Auxiliary page records (ezdgn, confirmed)

28-byte header: magic `0x0000A11B` `+0x00`, payload length `+0x04`, kind `+0x08`,
`+0x0C` reserved, element id `u64 +0x10`, flags `+0x18`; payload follows. cadkit only
counts them per element (`dgn.xattribute_count`, `dgn.xattribute_kinds`).

## V8 elements

### Common prefix and display header

| Offset | Type | Meaning | Evidence |
|---|---|---|---|
| `0x00` | u32 | type (low 16 bits) + flags (high 16) | all samples |
| `0x04` | u32 | total length in 16-bit words | all samples |
| `0x08` | u32 | attribute (linkage) offset in words | all samples |
| `0x0C` | u32 | level id (graphic) / table number (types 95, 96) | oracle: level 64 = "Default", level 1 on the first element |
| `0x10` | u64 | element id (unique per file) | dependency linkages resolve to these ids |
| `0x18` | f64 | last modified, milliseconds since 1970 (inferred: same value on every element of a save; 2017 in test_dgnv8, 2014 in TreeText) | — |
| `0x20` | u32 | graphic group | oracle row 1 (= 2) |
| `0x24` | u32 | unknown (`0x80000000` in test_dgnv8, 0 elsewhere) | — |
| `0x28` | u32 | property flags: `0x0800` 3D, `0x8000` hole (shapes), `0x0200`/`0x0400` new/modified (as V7), `0x0080` set on hidden tags (inferred) | oracle: Z vs 2D geometry; hole ring in a cell |
| `0x2C` | u32 | line style | oracle (4) |
| `0x30` | u32 | line weight | oracle (5) |
| `0x34` | u32 | color index (>255 = extended color) | oracle (3, 256) |
| `0x38` | 6 × i64 | element range: low corner xyz, then the extent (high − low) xyz, in UOR, for every graphic type (text, tags and cell headers included); only the model header (`Dgn~Mh` `0x90`) stores absolute low/high corners | public test_dgnv8 (ODA): every line, line string and shape, including a zero-length line stored with extent (0, 0, 0) (`tests/v8_range.rs`); Water and the MicroStation 8.11 files of the private corpus agree on every record. An absolute high corner reads as a negative extent wherever the geometry lies below zero, and MicroStation does not draw such elements |

Type-word flags (high 16 bits): `0x2000` complex header, `0x4000` complex component,
`0x1000` set on graphic elements, `0x0080`/`0x0040`/`0x0400`/`0x0800` vary by writer
(meaning unknown, kept in `ElementHeader::type_flags`). Types without this display
header: 1, 5, 8, 9, 10, 39, 57, 63, 66, 90–93, 95–99.

### Element bodies (offsets from the element start)

2D variants store 2 doubles per point and a rotation angle (radians, CCW); 3D variants
store 3 doubles and a quaternion `(w, x, y, z)`. All coordinates are UOR doubles.

| Type | Layout | Evidence |
|---|---|---|
| 3 line | `0x68` start, then end | oracle: 4 lines, 2 zero-length lines reported as POINT |
| 4, 6, 11, 21 | `0x68` u32 count, `0x6C` pad, `0x70` points | oracle; shape outlines repeat the first point |
| 22 point string | as 4, then `count × 4` doubles (orientation quaternions) | oracle MULTIPOINT |
| 15 ellipse | `0x68` primary axis, `0x70` secondary; 2D: `0x78` rotation, `0x80` center; 3D: `0x78` quaternion, `0x98` center | oracle (stroked points on the curve within 1e-6) |
| 16 arc | `0x68` start, `0x70` sweep (signed radians), `0x78` primary, `0x80` secondary; 2D: `0x88` rotation, `0x90` center; 3D: `0x88` quaternion, `0xA8` center | oracle incl. negative sweep in a complex shape |
| 17 text | `0x68` u32 font, `0x6C` u16 justification, `0x6E` u16 payload byte length (no terminator counted), `0x70`/`0x78` width/height multipliers (UOR = value × 6/1000), `0x80`/`0x88` measured length/height in UOR (0 when not measured; TreeText 583.33/50 for 12 characters of width 50, the same values as the range extent); 2D: `0x90` rotation, `0x98` origin = lower-left corner, `0xA8` u16 editable fields, `0xAA` text; 3D: `0x90` quaternion, `0xB0` origin, `0xC8`, `0xCA` text | oracle: value, size 1.0, angle −45°, Arial (font 1024); range low = origin on left-top texts |
| 7 text node | `0x68` u32 components, `0x6C` u32 node number, `0x70` u32 font, `0x74` u16 max length, `0x76` u16 justification, `0x78` line spacing, `0x80`/`0x88` multipliers; 2D: `0x90` rotation, `0x98` origin; 3D: `0x90` quaternion, `0xB0` origin | TreeText (font/justification equal the child text's) |
| 12, 14 complex | `0x68` u32 component count | oracle |
| 18, 19 | assumed as 12/14 (unverified) | — |
| 2 cell, 34 shared cell definition, 35 instance | `0x68` u32 components, `0x6C` u32 1 in every ODA and MicroStation cell (meaning unknown); 2D (primary length `0xC0`): `0x70` range low, `0x80` high, `0x90` 2×2 matrix, `0xB0` origin; 3D (`0x100`): `0x70` low, `0x88` high, `0xA0` 3×3 row-major matrix, `0xE8` origin; name = string linkage 1 | test_dgnv8: identity matrices, instance matrix `10000·I` at origin (0,1,2) = oracle POINT Z; definitions are stored in the 3D layout even with the 2D flag, so the variant is chosen by length |
| 27 B-spline curve | `0x68` u32 components, `0x6C` u8 order−2 (low nibble) + flags (`0x10` curve display, `0x20` polygon display, `0x40` rational, `0x80` closed), `0x6D` u8, `0x6E` u16 (unknown), `0x70` u32 poles, `0x74` u32 knots (0 = uniform) | oracle: cubic, see open question on "closed" |
| 26, 28 knots / weights | doubles after the `0x20` prefix (unverified, no sample) | — |
| 37 tag | `0xA0` origin (3 doubles), `0xB8` offset (3 doubles), `0xD0` u16 tag number, `0xD2` u16 value type, `0xF0`/`0xF8` text multipliers, `0x138` u32 value length, `0x140` value; set = dependency `0x2717`, target = dependency `0x2710`; displayed tags: see "Displayed tags" below | TreeText (13,808 tags, 2D) and private sample (418 tags, 3D) — same offsets for 2D and 3D |
| 39 tag set definition | `0x28` magic `teSt`, `0x2C` u32 (`0xF81` in every set), `0x30` u32 (varies), `0x34` u32 definitions length, `0x38` u32 same, `0x3C` definitions (V7 layout below), set name = string linkage 1 | TreeText (1 set), private sample (8 sets) |
| 94 raster frame | `0x78` 4×4 row-major pixel-to-UOR matrix (translation in column 3; observed: 62.65 UOR/pixel, translation = range low), `0xF0` m33 = 1, `0x108`/`0x110` frame extent from the translation (UOR, = the element range extent), `0x118`/`0x120` 200/200 (DPI?), `0x140` u32 768, 768 (unknown); the same values are repeated in a type-91 control record (`0x28` low corner, `0x40` corner, `0x60`/`0x68` DPI, `0x88`/`0xA8` scale) | private sample (8 frames of one raster file: one size, eight places) |
| 90 raster attachment (control) | string linkage 3 = file name (8-bit), `0x1F` = full path/URL (UTF-16) | private sample |
| 92 raster link (control) | `0x30` u64 id of the type-90 element; dependency `0x271B` (root type 3) → frame (94) id | private sample (all 8 frames resolve) |
| 96 table header | `0x0C` table number (1 levels, 2 fonts, 4 level filters, …), `0x20` u32 entry count | all V8 samples |
| 95 table entry, table 1 (level) | `0x20` u32 level id, `0x24` u32 parent (`ffffffff` none), `0x28` u32 flags (raw), name = string linkage 1, description = 2 | test_dgnv8 ("Default" = 64), TreeText (2 levels), private (370) |
| 95 table entry, table 2 (font) | `0x28` u32 font number, `0x2C` u16 name bytes, `0x2E` UTF-16 name | test_dgnv8 ("Arial" = 1024) |
| 5 color table (level slot 1) | `0x20` u16 screen flag, `0x22` 256 RGB triples, first = color 255 (V7 order assumed), then u16 length + table file name | private sample (layout of the triples inferred from the trailing name) |
| 36 | multi-line (assumed); not decoded → Unknown | oracle shows a 1-point line string |
| 66 (`Dgn~Mh`) | model header, below | all |

### Tag set definition records (V7 type 66 level 24 and V8 type 39)

Repeated until the declared length: name, `u16` tag number, prompt, `u16` value type,
5 bytes (`u16` default length? / `u16` type again / flag byte — not interpreted),
default value (type 1: string; 3, 5: 4 bytes; 4: double; other: 4 bytes). Strings are
either 8-bit NUL-terminated or `ff fd` + UTF-16LE terminated by a 16-bit NUL (seen for
non-ASCII names in the private sample).

### Linkages (attribute data after `2 × attr_words`)

Framing: byte 0 = words following the first word (`size = 2 × (byte0 + 1)`), byte 1
flags (`0x10` user data), `u16 +2` linkage id. V8 zero padding ends the area. V7 `00 00`
/ `00 80` starts an 8-byte DMRS database linkage.

| Id | Layout | Evidence |
|---|---|---|
| `0x56D2` string | `+4` u32 string id, `+8` u32 byte length (no terminator counted), `+0C` bytes (`ff fd` = UTF-16LE, else 8-bit) | model/cell/level/tag-set names, raster paths |
| `0x56D0` dependency | `+4` u16 application id, `+6` u16 value, `+8` u8 copy option, `+9` u8 root type, `+0A` u16 root count; root type 2/3: u64 ids from `+0C`; type 9/10: 16-byte roots, id in the second half (`+14`). MicroStation 8.11 tag targets: value 1, copy option 2, root type 9, root `01 00 00 00 00 00 01 00` + id, then 28 zero bytes (56 bytes in all) | tags (`0x2717` → set, `0x2710` → target), rasters (`0x271B`) |
| `0x0041` fill | V7: fill color byte `+8`; V8: u32 `+8` (meaning of the value unverified) | test_dgnv8, smalltest |
| `0x7D2F` association id | `+4` u32 | GDAL |
| `0x80D4` | `07 10 d4 80 00 02 00 00 00 00 00 00`, then `+0C` u32 code page: 1252 on every TreeText text, 1254 on the texts of Turkish MicroStation 8.11 files; taken as the text's code page | TreeText, private corpus |

String ids seen: 1 name, 2 description, 3 file name, `0x13` master unit label, `0x14`
sub unit label, `0x1F` full path.

### Displayed tags (type 37)

MicroStation 8.11 writes every displayed tag of the private corpus (68,745 tags in 113
files) with the layout below; offsets from the element start.

| Offset | Content |
|---|---|
| `0x00` | type word `0x10C0_0025`; property word `0x0E00` (3D); own level, named after the tag (e.g. `OdaAdi` for "Oda Adı"), not the target's; graphic group shared by the tags of one target |
| `0x38` | range: the text box around the display point, `2 × height` tall (low corner + extent like every graphic) |
| `0x98` / `0x9A` | u16 3 / u16 11 (TreeText's hidden tags: 3 / 3) |
| `0xA0` | origin: the target's first vertex |
| `0xB8` | offset from the origin to the display point (z 0) |
| `0xD0` / `0xD2` | tag number / value type |
| `0xD4` | u32 `0x02000000` on most tags (meaning unknown) |
| `0xF0` / `0xF8` | width / height multipliers (UOR = value × 6/1000) |
| `0x100` | quaternion; `(-1, 0, 0, 0)` when unrotated |
| `0x12C` / `0x130` | u32 font number / u32 justification (7 = center-middle) |
| `0x138` / `0x140` | value length / value: 8-bit text in the locale code page with a counted NUL, i32, or f64 |
| after the value | 10 bytes, then the linkages at a 2-byte boundary: set dependency (24 bytes), target dependency (56 bytes, root type 9) |

Hidden tags written by MicroStation (TreeText) leave the range empty.

### Model index (`Dgn~Mix`, ezdgn, confirmed)

Header: magic `0xAA00BA11`, version 4, entry count, u32. Entries: `+0` u32 (low 16 =
storage index, high 16 = model number), `+4` flags, `+8` u64 (equals the model header's
`0x18` timestamp), `+10` u16 entry length, `+12` u16 name bytes, `+14` u32 description
bytes, `+20` UTF-16 name then description.

### Model header (`Dgn~Mh`)

`0x1000` bytes of (mostly zero) data, a zero `u32`, then one type-66 element ending at the
stream end (found by scanning, as ezdgn does).

| Offset | Type | Meaning | Evidence |
|---|---|---|---|
| `0x00` | u32 | type word; `0x00800000` set = 2D model | Safe files 2D, test_dgnv8/private 3D |
| `0x18` | f64 | timestamp (as elements) | |
| `0x48`, `0x4C` | u32 | unit flags (`0x11` in all samples) | |
| `0x50`, `0x58` | f64 | master unit numerator / denominator (units per meter; m = 1/1) | m in all samples |
| `0x60`, `0x68` | f64 | sub unit numerator / denominator (mm = 1000/1) | test_dgnv8 (mm), Safe (m = 1/1) |
| `0x90` | 6 × i64 | model extents (UOR) | |
| `0xC8` | 3 × f64 | global origin (UOR; 0 in all samples) | |
| `0xE0` | f64 | UOR per master unit (10000 / 10) | oracle coordinates |
| `0xE8`… | f64 | further unit data (1, 1, 1e7, …; not decoded) | |

Master coordinates: `(uor − global_origin) / uor_per_master` (confirmed by the oracle).
Size of a unit in meters = `denominator / numerator` (consistent with m and mm; feet
not observed).

## V7 (ISFF)

Record header: byte 0 level (low 6 bits) | `0x80` complex; byte 1 type (low 7 bits) |
`0x80` deleted; u16 words to follow; end marker `ff ff`. 32-bit integers are two LE words,
high word first; doubles are VAX D-float (converted exactly: exponent − 128 → IEEE + 894,
fraction truncated by 3 bits). Display header (most types): range 6 × offset-binary i32
at 4, graphic group u16 28, attribute index u16 30 (linkages at `32 + 2·index` when
properties bit `0x0800` is set), properties u16 32, symbology u16 34 (style bits 0–2,
weight 3–7, color byte 35). File header `08|C8 09 FE 02` (C8 = 3D) or cell library
`08 05 17 00`.

| Element | Layout (GDAL dgnlib) |
|---|---|
| 9 TCB | 3D flag byte 1214 & `0x40`; sub/master i32 1112; UOR/sub i32 1116; labels 1120, 1122; origin 3 × VAX at 1240 (UOR) |
| 5 level 1 color table | screen flag 36; RGB 38 = color 255; 41.. colors 0–254 |
| 3 line | points at 36 (2D 8 bytes, 3D 12 bytes per point) |
| 4, 6, 11, 21, 22 | u16 count 36, points 38 |
| 15 ellipse | VAX axes 36/44; 2D: rotation i32 (1/360000°) 52, VAX center 56; 3D: quaternion 4 × i32 52, center 68 |
| 16 arc | start i32 36, sweep sign-magnitude 40 (0 = full), VAX axes 44/52; 2D rotation 60, center 64; 3D quaternion 60, center 76 |
| 17 text | font 36, justification 37, multipliers 38/42; 2D: rotation 46, origin 50, length u8 58, text 60; 3D: quaternion 46, origin 62, length 74, text 76; `ff fd` = 16-bit units |
| 7 text node | total words 36, count 38, node 40, font 44, justification 45, spacing 46, multipliers 50/54; 2D rotation 58, origin 62; 3D quaternion 58, origin 74 |
| 12, 14, 18, 19 | total words 36, count 38 |
| 2 cell | total words 36, RAD-50 name 38/40; 2D range 52/60, matrix 4 × i32 68 (scale `10000 / 2^31`), origin 84; 3D range 52/64, matrix 76, origin 112 |
| 27 B-spline curve | description words i32 36, order/flags 40, curve type 41, poles u16 42, knots u16 44 |
| 26, 28 | i32 values from 36 / (2^31 − 1) |
| 34, 35 shared cell (ezdgn, 2D) | VAX 2×2 transform 76, origin LE i32 148, name 16 bytes 164 |
| 37 tag | set u32 68, tag number 72, type 74, length 150, value 154 |
| 66 level 24 tag set | count 44, flags 46, name 48, definitions follow; set number in an `03 10 2F 7D` linkage |

Complex grouping: a header's components are the following records with the complex bit
inside `offset + 38 + 2·total_words` (B-spline headers: `offset + 40 + 2·desc_words`)
(ezdgn). The first TCB wins (smalltest has two type-9 records). Deleted records are
skipped (counted in `dgn.deleted_count`). Records without a display header and types 5,
8, 66 are design data, not entities.

## Mapping decisions

- **Coordinates**: UOR → master units of the first model (`Document::units`); other models
  are rescaled when both unit sizes are known (`dgn.rescaled_to_document_units`).
  Shared cell definitions use the same scale without the global origin.
- **Units**: master unit definition → `meters_per_unit` and the matching `LengthUnit`
  (else `Custom`); V7 and fallback: the master label (`m`, `mm`, `ft`, …).
- **Colors**: 0–255 through the file color table (last one wins) or the default palette →
  `Color::Rgb`; index in `dgn.color_index`. Extended colors (>255) → `ByLayer` (their RGB
  is not located; the oracle reports different RGBs for the same index in two exports).
- **Weights**: weight `w` → `Lineweight::Millimeters(0.1 · (1 + 2w))` (a weight-`w` line is
  `1 + 2w` pixels wide; 0.1 mm per pixel assumed); raw value in `dgn.weight`.
- **Styles**: 0–7 → `DGN_SOLID`, `DGN_DOTTED`, `DGN_MEDIUM_DASH`, `DGN_LONG_DASH`,
  `DGN_DOT_DASH`, `DGN_SHORT_DASH`, `DGN_DASH_DOUBLE_DOT`, `DGN_LONG_DASH_SHORT_DASH`;
  larger values `DGN_STYLE_n` (custom styles, no pattern); linetype entries have empty
  patterns.
- **Text**: the stored origin is the **lower-left corner** of the text box for every
  justification. Evidence: GDAL dgnlib documents it ("Bottom left corner of text") and its
  writer computes text bounds from it; smalltest's center-center text has origin x = range
  left edge; test_dgnv8's left-top texts have range low = origin. The justification code
  becomes `halign`/`valign` (0–2 left, 3–5 left margin → left, 6–8 center, 9–11 right
  margin → right, 12–14 right; top/center/bottom → `Top`/`Middle`/`Baseline`, the bottom
  of the DGN text box being the baseline), and `position` is moved from the origin to the
  justification point: `origin + x̂·(fx·L) + ŷ·(fy·H)` with `fx, fy ∈ {0, ½, 1}`,
  `H` = text height and `L` = text length — V8 `0x80` when non-zero (MicroStation writes
  it), V7 range width for unrotated text whose range starts at the origin, otherwise
  `characters × character width` (GDAL's estimate; `dgn.text_length_estimated = true`).
  `end_point = None`. The stored origin is kept in `dgn.origin`, the length in
  `dgn.text_length`, the code in `dgn.justification`. Width factor = width / height.
  Style = font name (font table) or `DGN_FONT_n`; one `TextStyle` per font. Checks:
  smalltest's center-center anchor equals the range center within 1e-6; the oracle texts
  (all left-top) keep their CSV point in `dgn.origin` with the anchor one height above it
  along the rotated Y axis; all 12,623 TreeText texts are left-bottom (anchor = origin,
  measured length present).
- **Text nodes** → `Group { TextNode, origin }` with one `Text` per line (keeps per-line
  positions; `MText` would lose them).
- **Ellipses / arcs**: `Circle`/`Arc` when the axes are equal, else `Ellipse` (major axis =
  longer axis; parameters shifted by π/2 when the secondary axis is longer). Clockwise
  sweeps become the equivalent CCW range. Quaternions: GDAL convention (stored
  `(w,0,0,z)` with `z = sin(−θ/2)` is a CCW rotation by θ).
- **Curves (11)** → `Spline` with `fit_points` = stored points minus the two tangent points
  at each end (oracle: stroke starts at point 3, ends at point n−2); all points in
  `dgn.curve_points`.
- **B-splines (27 + 21/26/28)** → `Spline`; uniform knots: clamped for open curves,
  periodic curves unwrapped into an open NURBS (poles + first `degree` poles, knots
  0..n+order) so standard evaluation traces the closed curve.
- **Cells (2)** → `Group { Cell, name, origin }`. **Shared cell definitions (34)** →
  `Block` (name from string linkage 1, base point = origin); **instances (35)** → `Insert`
  with scale = matrix column lengths, rotation/normal from the matrix (raw matrix in
  `dgn.matrix`).
- **Complex chains/shapes** → `Group { ComplexChain | ComplexShape }`; 18/19 and unknown
  headers with components → `Group { Other }`.
- **Line strings / shapes (4, 6)** → `Polyline` (`normal = +Z`: DGN has no bulges); shapes
  are `closed` and drop the repeated first vertex. A zero-length line (3) is a `Point`
  (the oracle reports it as POINT).
- **Point strings (22)** → `Group { Other, name "point string" }` of `Point`s.
- **Tags** → `Attribute { tag: definition name, value, set: set name, position: origin +
  offset, invisible: property bit 0x0080, layer: the tag element's level when it is not
  the target's, display: size, font style, alignment and rotation of displayed tags,
  props: symbology and flags that differ from the target's }` on the target entity (looked up by element
  id, also inside groups). Tags whose target is missing (and V7 tags) stay as
  `Unknown { "dgn.type_37" }` entities carrying the attribute (`dgn.tag_target_missing`).
- **Raster frames (94)** → `Image`: `position` = matrix translation, `u_vector` /
  `v_vector` = the matrix X / Y columns scaled so the frame spans its extent
  (`0x108`/`0x110`, relative to the translation), which handles rotated or sheared frames. Consistency rule: the four
  corners must lie inside the element range (MicroStation's own bounding box) within
  4 UOR; otherwise the rectangle is rebuilt from the range — along the matrix axes when
  that is solvable, else axis-aligned (`dgn.raster_outside_range`); without a transform
  the range is used (`dgn.raster_range_placement`). Props: `dgn.range` (element range in
  document units), `dgn.raster_matrix`, `dgn.raster_file`, `dgn.raster_path`. Path =
  attachment full path, else file name. `size_px` stays `None` (pixel counts live in the
  external raster).
- **Everything else** → `Unknown { "dgn.type_N" }` with `dgn.*` props; `keep_raw` stores the
  exact element bytes. Undecodable bodies keep their header/symbology and get
  `dgn.decode_problem`.
- Warnings use stable codes: `dgn.page_records`, `dgn.page_count`, `dgn.zlib`,
  `dgn.stream_unreadable`, `dgn.model_header`, `dgn.model_index`, `dgn.v7_truncated`,
  `dgn.v7_units`, `dgn.unknown_element.type_N`, `dgn.decode_failed.type_N`,
  `dgn.tag_target_missing`, `dgn.raster_unlinked`, `dgn.bspline_knots`, `dgn.depth_limit`
  (repeated issues are aggregated with a count).

## Tag sets in props

The model has no table for attribute definitions, so every tag set is stored in
`Document.props["dgn.tag_sets"]` (V8 and V7) as positional lists:

```text
[                                   one entry per tag set, sorted by key
  [ name | null,                    set name (string linkage 1 / V7 name)
    key,                            V8: definition element id; V7: set number
    [                               one entry per tag definition
      [ number, name, prompt, value type (1 text, 3 int, 4 double, 5 binary),
        default (Text / Int / Float / Bytes),
        flags (Bytes: the 5 uninterpreted definition bytes) ],
      ...
    ] ],
  ...
]
```

## Strings

`ff fd` + UTF-16LE (V8 string linkages, tag-set names, V7 multibyte text); `ff fe 01 00`
+ 8-bit text (V8 text, test_dgnv8 "myTéxt"); otherwise 8-bit. 8-bit text is decoded with
the element's `0x80D4` code page when present, else `ReadOptions::fallback_codepage`
(default windows-1252). No UTF-8 sniffing. The private sample stores non-ASCII tag values
as 8-bit text without any code page information in the file, so callers must pass the
right `fallback_codepage`. Searched for evidence of a file-level code page: SummaryInfo
(code page 1200 = the property set's own UTF-16, locale 0x409 en-US in every sample),
`Dgn~H` (1576 bytes; non-zero fields are small counters, an id high-water mark and the
timestamp), the type-9 settings element and every inflated stream: the only code-page
values are the per-text `0x80D4` linkages (12,627 in TreeText for 12,623 texts); the
private sample contains none (it has no text elements).

Lengths never count a terminator in text element payloads (`0x6E`) or string linkages:
TreeText, test_dgnv8 and the private corpus agree, and MicroStation draws a counted NUL as
an extra glyph at the end of the text. Tag values do count it (TreeText 13 bytes for 12
characters), and tag set definition strings are NUL-terminated.

## Verification

| Check | Result |
|---|---|
| test_dgnv8 vs `test_dgnv8_ref.csv` (34 features) | 34/34: type, level, color, weight, style, graphic group equal; every oracle point lies on the mapped geometry within 1e-6 (points, lines, line strings, multipoints, ellipses, arcs, complex chains/shapes incl. arcs, polygons with a hole, text origin/value/size/angle, shared cell origin); curve and B-spline start/end equal (B-spline evaluated with cadkit's NURBS) — `tests/v8_oracle.rs` |
| smalltest (V7) vs GDAL values published by ezdgn | text "Demo Text" at (0.7365, 4.2198); ellipse center (5.0082, 4.5835), radius 4.679606584; shape vertices; line end points — `tests/v7_corpus.rs` |
| seed_2d VAX origin | (−249879416, −669487710) UOR — `tests/v7_corpus.rs` |
| TreeText | 6,904 text nodes, 12,623 texts, 13,808 tags all attached (2 per node, set "Trees", tags "Species" text / "Tree Count" integer) |
| Water | 66,053 elements (66,035 line strings, 18 lines), no warnings |
| private sample | 7 graphic pages, 2,032 records (1,533 shapes, 418 tags, 49 ellipses, 24 line strings, 8 raster frames); 418/418 tags attached with resolved set and tag names; 8/8 rasters linked to a path; 370 levels; no Unknown entities |
| robustness | truncations and bit flips of every sample (`tests/robustness.rs`); mutations of the inflated pages repacked into new compound files (`src/fuzz_tests.rs`); V7 body stamping |

## Open questions

1. **Level flags** (`0x28` of level entries) are not decoded; layers are reported
   visible, unfrozen, unlocked, with the raw value in `dgn.level_flags`. Observed:
   test_dgnv8 "Default" (used) `0x02600002`; TreeText and Water "Default" (unused)
   `0x026F0002` and the used level `0x02670002`; private sample: `0x026F0006` on all 370
   levels (122 used, 248 unused) — so no bit separates any observable state there.
   104-byte entries (public files) have `u32` 1 at `0x3C`, `0x4C`, `0x5C` on the used
   level and 0 on "Default"; the 240-byte entries (private) have a value at `0x5C` that is
   distinct for every level and is not the elements' color. Level symbology offsets are
   therefore unknown.
2. **B-spline "closed"**: both test_dgnv8 curves have `0x6C = 0x22` (bit `0x80` clear) and
   `0x6D = 0x01`, yet the oracle strokes them as periodic. cadkit treats bit 0 of `0x6D`
   as "closed" in V8; a sample with an open non-uniform curve would settle this.
3. **Extended colors** (index > 255): RGB source not found in the file.
4. **Sheet models**: no public sample with a sheet model was found, and every sample has
   one design model, so no field can be tied to design/sheet. Every model is reported as
   `ModelKind::Model` (`dgn.model_index_flags`, `dgn.model_type_word` kept); 2D/3D comes
   from bit `0x00800000` of the model header type word.
5. **Storage vs master unit**: `0xE0` is used as UOR per master (matches the oracle);
   `0xE8`/`0xF0` might be a storage unit definition — irrelevant while both are 1/1.
6. **Type 33 dimensions, 36 multi-lines, 26/28 V8 knots and weights, 18/19 bodies**: no
   samples; left Unknown / unverified.
7. **V7 tag targets**: not attached. GDAL dgnlib reads only the set number (68), tag number
   (72), type (74), length (150) and value (154) of a tag element and has no reader or
   writer code for the tag-to-element association; no public V7 sample contains tags.
8. **Hidden flag** `0x0080` on tags (TreeText) is inferred from one file.
9. **Text length estimate**: V7 rotated text and V8 text without a stored length use
   `characters × character width`, which is exact only for fixed-pitch fonts.
10. **Raster extent** (resolved): `0x108`/`0x110` hold the frame extent from its
    translation, the same values as the element range extent. The 8 frames of the private
    sample show one raster file at one size in eight places; reading the extent as an
    absolute corner had made them appear to share one corner. Rotated frames are not
    sampled. No public sample contains a raster frame.

## Seed-based V8 writer

`write_v8(Document, seed, WriteOptions)` constructs new model graphics in a
single-model seed. Source dimension and known master units must match the seed.
A plan may occupy a 3D model; the writer never silently changes the seed dimension
or drops Z. Nonempty seeds require `clear_seed_model=true` explicitly.
`repack_v8` reconstructs the CFB container without changing stream contents.
Neither API edits the input buffer or file.

The reader and writer use the same deterministic level-name assignment in table
order. Duplicate names receive a level-ID suffix, and literal names that already
look like such a suffix are disambiguated again. Empty/missing names use `Level N`.
The writer maps these neutral names back to the existing seed level IDs instead of
overwriting duplicate raw names or creating extra levels. Synthetic tests cover
duplicate, unnamed and suffix-colliding table entries and verify both membership
and the level-table count after readback.

3D rotations are encoded with the conjugate of the conventional local-to-world
quaternion, matching `native::element::Rotation::matrix` and the attributed GDAL
convention. The former unconjugated encoding reversed a synthetic +30-degree text
rotation and the Y component of an ellipse's major axis. Regression tests compare
all matrix-to-quaternion branches, text angles, ellipse axes and tilted normals;
2D angle encoding is unchanged.

The writer uses the MIT `cfb` 0.14 crate for CFB sector allocation and directory
construction. Existing streams and directory metadata are copied unless replaced
or explicitly removed. Fresh entry times derive from the seed root, making
repeated output deterministic. Model graphic/control/auxiliary pages are replaced;
named tables are retained and extended. Unknown seed streams are preserved, not
interpreted or certified as free of references to cleared graphics.

Streams are matched for replacement by their CFB names joined with `/`. The `cfb` crate
builds entry paths with the platform separator (`\` on Windows), so its path strings are
not used as keys; before this rule, Windows output kept every nested seed stream
unchanged and dropped new pages whose numbers collided with seed pages. A numbered
(`$n`) page the writer would replace but the reader could not decode is an error, since
its content could be neither checked for emptiness nor carried into the named tables.

| Written structure | Layout evidence |
|---|---|
| File header | Existing reader and own corpus analysis; zlib payload after `0x14`, expected 1,576 bytes; `0x128` high-water element ID updated |
| Model header | Existing decoded header bytes, including original stream prefix; model extent fields at `0x90` updated |
| Element pages | Existing page decoder, zero delimiter per record, bounded batches, zlib encoding |
| Element identity | 64-bit ID at `0x10`, allocated above both observed records and header high-water mark |
| Display/geometry | Existing V8 decoder layouts; finite UOR conversion, 64-bit range bounds |
| Levels, tag sets, tags | Existing decoded tables/linkages, seed templates when available, dependency target IDs |

File-header time and revision fields are preserved: their semantics have not
been independently established. Page version is taken from the seed; no universal
"last page flag" rule is assumed. A small set of matching local files is not a
complete binary format specification.

Supported new entities: line, straight line string, closed shape, circle,
ellipse, arc, left/center/right text at baseline/middle/top, complex chain/shape
and ordinary cell groups. Zero-width bulged polylines are decomposed into exact
line/arc components, with signed XY sweeps for clockwise 2D arcs; no tessellation
is involved. Parent attributes remain attached to the resulting complex group.
Faces and meshes become shapes/cells. Polygons with holes become a cell with
hole-marked shape components. Polygon rings must lie within
`WriteOptions::planarity_tolerance` (default 0.01 source units, 1 cm in metres) of the
exterior ring's plane; 1e-6 rejected every polygon with a hole in 0.1 mm-rounded
CityGML sources. Shared-cell instances, new/edited raster attachments, splines,
hatches, dimensions, fitted/oblique text and nonzero polyline widths require
explicit conversion and currently return `Unsupported`.

Text justification codes 0..14 are retained when they agree with the neutral
alignment. The writer inverts the reader's anchor shift using `dgn.text_length`
(measured baseline advance in drawing units) and character height. Center/right
text requires that measurement. Callers must refresh it after changing text,
font, height or width factor; cadkit does not measure fonts. Left-aligned text
without it uses the existing character-count estimate. Bottom alignment relative
to descenders is distinct from the DGN baseline and remains unsupported.

Every graphic record stores its range as the floored low corner and the extent to the
ceiled high corner, both in UOR; a negative or overflowing extent is an error. Before
0.3.0 the writer stored absolute high corners for everything but text, which MicroStation
reads as negative extents below zero and does not draw. Cell headers aggregate their
components the same way; their body repeats the absolute low/high corners (as ODA
writes them) and `0x6C` holds 1. The model header keeps absolute corners. Text ranges
enclose the rotated measured rectangle; measured advance and nominal height do not
promise exact target-font glyph bounds.

`preserve_seed_rasters` (default true) retains unchanged attachments from the same
seed. All image entities must have unique original IDs and unchanged geometry,
paths, display properties and native props. Frame records (94), raster control
records (90..93), and their auxiliary payloads retain their bytes and IDs; other
graphics and attached tags are regenerated. `preserve_seed_controls` (default true)
keeps every other seed control record (coordinate system, model settings and the like)
and its auxiliary data byte for byte, unless the record holds the id of a seed graphic;
the control auxiliary page counter is kept while all of its pages are. Incomplete auxiliary pages, mixed/new/deleted raster
sets, or edited attachment settings are rejected. An output is still dependent
on its external raster file; no raster bytes are embedded or fabricated. This is
retention of existing attachments, not a new native raster encoder.

Level names, tag sets and attached text/integer/double values are writable.
Unnamed tag sets remain unnamed rather than being silently renamed to `CADKIT`.
Standalone type-37 tags are written without an invented owner; their values and
positions are retained without creating a dangling dependency. An attribute with
`display` is written as a displayed tag in the layout of "Displayed tags": origin at
the owner's first vertex, offset to `position`, size, font (`display.style` resolved
by name like text styles, or `dgn.font_number`), justification and quaternion; its range
uses `dgn.text_length` when given, else characters × width. `Attribute::layer` places the
tag element on its own level, and the attribute's `dgn.color_index`, `dgn.weight`,
`dgn.style`, `dgn.graphic_group`, `dgn.properties` (`0x0600` bits) and `dgn.type_flags`
(`0x00C0` bits) props override the owner's. Owner dependencies use MicroStation's
56-byte root type 9 form. Every entity likewise reproduces `dgn.graphic_group` and the
`0x0600` / `0x00C0` flag bits the reader records.
Strings default to UTF-16; `codepage="windows-1254"` selects explicit legacy
Turkish text encoding and, on text elements, the code page linkage as MicroStation
writes it. Text payloads and name linkages count no terminator; tag values end with a
counted NUL. Names use UTF-16. Unrepresentable
characters fail. IDs and tag owner dependencies are assigned together. DGN tag
integers are restricted to the on-disk signed 32-bit range. A tag definition has one
value type, so when any integer value of a (tag set, tag) pair exceeds that range,
every value of that tag is written as a double (type 4); values beyond ±2^53, which
a double cannot hold exactly, fail.

ACI/RGB colors are matched exactly against the seed's palette. An unavailable
color fails unless the caller explicitly provides `dgn.color_index`. ByLayer
color uses a supplied neutral layer color or seed palette index zero when no
explicit color exists. This materializes the color, not a live inheritance rule.
ByBlock requires prior resolution. Native lineweight indexes must be in `0..31`;
an entity's explicit `dgn.weight` takes precedence over the neutral layer's
`dgn.weight`. A millimetre-only weight has no universal DGN index mapping and
requires explicit caller conversion. Nonzero font indexes must be in the seed's
font table or actually referenced by seed Text/TextNode records; font zero is
the default when a text names neither a style nor a number (before 0.3.0 the first
table entry by name was taken, an arbitrary seed font). A named style must agree with its numeric font index.
Line-style indexes must refer to the seed's tables. Existing seed level settings are retained;
writing arbitrary neutral level flags, custom fonts/linetypes, CAD props and
layout/application semantics is not implemented. This is new geometry export,
not general lossless Document-to-DGN roundtripping.

### Evidence and acceptance boundary

Synthetic tests cover 2D/3D models, Unicode and CP1254 tags/text, tag dependencies,
palette mapping, holes, wrapped arc angles, limits and truncated seeds. Public
GDAL `test_dgnv8.dgn` is tested for repack preservation, new geometry, deterministic
output and independent `cfb::CompoundFile::open_strict`. Existing public CSV
reader-oracle tests remain separate. Private seeds are checked in memory when
`CADKIT_PRIVATE_DGN` is supplied; they are not copied into fixtures or outputs.

These tests establish local container/reader agreement. They do not establish
MicroStation or CityMax acceptance, unknown-field correctness for all V8 files,
or byte-identical regeneration of vendor-written elements.

The reproducible public/synthetic acceptance kit is generated with:

```sh
python3 scripts/buildlock.py cargo run -p cadkit --example interchange -- \
  corpus/public/gdal/test_dgnv8.dgn target/interchange-validation
cadkit repack-dgn public-input.dgn repacked.dgn
cadkit convert geometry.dxf output.dgn --seed blank.dgn
```

Before claiming external acceptance, open the repacked file, check new and moved
geometry/levels, check attached tags, then save and reopen in the target software.
No external test environment is currently available. No compatibility
certification is claimed. The `dgn_write` fuzz target also exercises reading,
seed-based writing and repacking under reduced resource limits; short
coverage-instrumented AddressSanitizer runs complement the deterministic tests.

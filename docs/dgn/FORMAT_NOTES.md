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
| Levels → layers | level table (names, ids); 240-byte entries: color index (`0x48`) → layer color, weight/style kept raw; flags kept raw | used levels as `Level N` |
| Colors | default palette + type-5 color table; extended (>255) colors unresolved | default palette + type-5 color table |
| Elements | 2, 3, 4, 6, 7, 11, 12, 14, 15, 16, 17, 18/19 (group), 21, 22, 27, 34, 35, 37, 94 | same families except raster |
| Tags | tag sets (type 39) + tag elements attached to their target | decoded, **not attached** (no sample) |
| Unmodelled | `Unknown { type_name: "dgn.type_N" }` + props, never dropped | same |

## Sources

Clean-room: no ODA/Bentley SDK material, no GPL code. Sources used:

| Source | License | Used for |
|---|---|---|
| `[MS-CFB]` Compound File Binary format (public Microsoft open specification) | open spec | CFB header, FAT/DIFAT, directory (2.6.1), free directory entries (2.6.3 "Other Directory Entries"), sibling tree order and colours (2.6.4 "Red-Black Tree"), mini stream |
| `[MS-OLEPS]` property sets (public Microsoft open specification) | open spec | `\u0005SummaryInformation` (application name, code page) |
| ezdgn <https://github.com/monozukuri-ai/ezdgn> `docs/v8/FORMAT_NOTES.md`, `crates/ezdgn-core/src/v8/*.rs`, `src/entities.rs` | MIT | V8 page/record framing, model index and model header search (`find_model_header`), common header, line/vertex/ellipse/arc/text/text-node offsets, string-linkage framing; V7 description-length rule (`container_descriptor`), V7 shared cell offsets |
| GDAL dgnlib `ogr/ogrsf_frmts/dgn/{dgnread.cpp,dgnhelp.cpp,dgnopen.cpp,dgnlib.h}` <https://github.com/OSGeo/gdal> | MIT/X | V7 record header, display header, TCB, color table, every V7 element layout, tag set / tag value layout, default 256-color palette (`abyDefaultPCT`, ported to `src/palette.rs`), RAD-50, linkage framing, quaternion convention; `DGNPF_*` property flags (no meaning for `0x0080`, so V7 visibility is unchanged) |
| GDAL dgnlib `ogr/ogrsf_frmts/dgn/dgnstroke.cpp` (`DGNStrokeCurve`), `dgnwrite.cpp` (`DGNCreateComplexHeaderFromGroup`), `ogrdgnlayer.cpp` (complex header children) | MIT/X | type 11 curve end tangents (Akima slope at the first and last fit points, `map::curve_end_tangents`); complex chain/shape headers own plain components (count excludes the header; the reader expects no further header among the children) |
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
color `0x43`, left/right/child `0x44`/`0x48`/`0x4C`, start sector `0x74`, size `0x78`;
the high size dword is ignored for version 3). Streams below the cutoff live in the mini
stream (the root entry's chain). Most observed DGN files are version 3; some seeds are version 4
(4096-byte sectors), where the 512-byte header is padded to fill sector -1, so sector `n`
starts at `(n + 1) * 4096`.

Directory slot order and sibling tree shape differ between writers. Readers find entries
through the sibling trees, so neither affects a lookup. Sibling order ([MS-CFB] 2.6.4):
shorter names (in UTF-16 units) first, then the characters compared after the simple case
conversion, as the `cfb` crate compares them (it checks the order when it opens a file, so
the writer must agree with it): a character maps to its uppercase only when that is one
character (`ß`, `ŉ`, ligatures and Greek letters with breathing marks stay as they are),
the Greek letters with ypogegrammeni map to their titlecase form (`ᾀ` → `ᾈ`), the letters
that received an uppercase in Unicode 16 stay unchanged (the crate's ICU data predates
them), and a supplementary character compares by code point, not by UTF-16 unit
(`cfb_tree::cmp_cfb_name`; before, the first character of the full uppercase was compared
per UTF-16 unit, so a seed with a stream named, say, `ß…` next to `T…` was rewritten into
a file the `cfb` crate rejects). ASCII names order as before. Structural counts
(`tests/structure.rs`):

| Property | Private corpus (567 files, all version 3) | Public V8 samples (test_dgnv8, TreeText, Water) |
|---|---|---|
| Slots 1–3 hold `Dgn~H`, `Dgn~S`, `Dgn-Md` | 277 files (290 start with `Dgn~S`) | none: test_dgnv8 and Water start `Dgn^Ix`, `Dgn~Mix`, `Dgn-Md`; TreeText (saved by MicroStation 8.11) `Dgn~H`, `Dgn~S`, `Dgn~Mf` |
| Every entry in the writer's slot order (below) | 0 files | 0 files |
| Sibling trees that are ordered red-black trees | 5,363 of 5,372 storages (9 files have one ordered tree with invalid colours) | 27 of 27 |
| Trees shaped by top-down 2-3-4 red-black insertion in slot order | 5,323 of 5,372; every tree in 524 files | 25 of 27 (Water 8 of 10) |
| Root entry colour 0 | 567 files | 3 files |
| Unused slots in the [MS-CFB] 2.6.3 free form | 823 of 829 (6 in 5 files differ) | TreeText 3 of 3; test_dgnv8 0 of 3 and Water 0 of 1 (object type 0 and name length 0, but stale bytes elsewhere) |

The writer orders the directory it builds by `cfb_tree::writer_slot_rank` (`Dgn~H`,
`Dgn~S`, `Dgn-Md` at slots 1–3, every storage before its children, `^AH` before `$1`),
the order of one seed lineage of the private corpus, not a rule other files follow; it
rebuilds every sibling tree with top-down 2-3-4 insertion in slot order, keeps the seed's
root colour, and writes every slot the root does not reach in the free form of [MS-CFB]
2.6.3 ("Other Directory Entries": object type 0, all bytes zero except the left sibling,
right sibling and child ids, which hold `0xFFFFFFFF`). The `cfb` crate writes such slots
with a name length of 2; outputs before this change kept that form. `cfb::CompoundFile::open_strict`,
cadkit's reader and the writer itself (using the output as a seed) accept the free form
(`writer/container.rs` tests); the public TreeText sample has it too.

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
| `\u0005SummaryInformation` | OLE property set: PID 1 code page (1200 observed), PID `0x12` application (`VT_LPWSTR` in the observed files; cadkit writes `VT_LPSTR`), PID `0x80000000` locale; see "Property sets" under "Seed stripping" | — |
| `.Embedded/.Index`, `Oda~SH`, `DgnTk`, other `\u0005…` | not decoded | — |

`$n` page numbers sort numerically (`$10` after `$9`). `#NNNNNN` is the decimal storage
index. Pages of 16 bytes have no payload (record count 0).

### Page stream

| Offset | Type | Meaning |
|---|---|---|
| `0x00` | u32 | record count |
| `0x04` | u32 | page format version (2 or 3 observed) |
| `0x08` | u32 | page number: 0 on 8,905 of 8,905 pages of the private corpus; on the public samples the page's own `n` on graphic, control and dictionary pages that store one (test_dgnv8, Water, and the graphic page of TreeText, saved by MicroStation 8.11), another non-zero value on their auxiliary pages |
| `0x0C` | u32 | population: 0 on the same 8,905 private pages; the record count on every public page that stores a page number |
| `0x10` | zlib | records |

Pages that store numbers: test_dgnv8 3 of 4, Water 137 of 138, TreeText 1 of 4
(`check_structure`, `pages_with_number_or_population`). Readers ignore both fields; the
writer stores 0 in the pages it writes and keeps unchanged seed pages as they are. A page
without records is the 16-byte header alone: every empty page observed (private corpus 644
graphic and 1,087 dictionary pages; public: one `Dgn^GA` and one `Dgn^CA` page). The writer
does the same (0.5.3); before, it appended a zlib stream of no data to the header.

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

### Dictionary pages (`Dgn^Nm`, `Dgn^NmA`)

A complex element is a record without the component bit (`0x4000_0000` in the type word)
followed by every record that has it: a table header (type 96, header bit `0x2000_0000`)
with its type-95 entries, a cell definition with its components. Structural counts over
the private corpus (567 MicroStation files, 3,932 dictionary pages; `tests/private_dictionary.rs`
asserts the rules on the 274 plan files and on what the writer makes of them):

| Rule | Evidence |
|---|---|
| A complex element never spans pages: no page starts with a component | 0 of 3,932 pages |
| A table header's `0x20` equals the number of components that follow it on its page | 5,425 of 5,425 tables |
| A level table (type 96, table 1) holds only level entries (type 95, table 1) | 918 of 918 tables |
| Level table flags come in pairs: header `0x2440` / entries `0x4440` or header `0x2480` / entries `0x4480` (high 16 bits) | 568 + 350 tables |
| The level table header need not open its page; pages hold several tables and tag sets | 422 of 918 at index 0 |
| A page has no size bound of its own | every file has a page over 256 KiB; largest 595,424 inflated bytes (1,356 records) |
| Empty pages (16-byte header, no payload) occur | 1,087 of 3,932 pages |
| Page format versions 2 and 3 both occur; each page keeps its own | 3,364 version-2 and 568 version-3 pages |
| Page number and population (`+0x08`, `+0x0C`) are 0 | 3,932 of 3,932 `Dgn^Nm` and 1,188 of 1,188 `Dgn^NmA` pages (the public test_dgnv8 and Water samples store the page number and record count on theirs, see "Page stream") |
| Every `Dgn^NmA/$n` XAttribute belongs to an element of `Dgn^Nm/$n` | 4,215 of 4,215 records |
| `Dgn^NmA/^AH` (meaning unknown) never exceeds the highest dictionary page number | equal in 91 files, lower in 476 |

MicroStation does not join a complex element across pages. A cadkit output whose
414-entry level table was split (216 entries on the header's page, 198 on the next), once
saved by MicroStation, had the component bit cleared on the 198 entries of the
continuation page (`0x4440` → `0x0440`) while the entries on the header's page and the
entries of an intact table on the same page kept it. In a field test MicroStation showed a
0.3.4 output (level table split 102 + 228) on a single level, and the same file with the
seed's own dictionary pages restored showed the full level list. Which of the three
differences (the split, the page number and population, the page numbering) decided it
was not separated; the writer removes all three.

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
| `0x28` | u32 | property flags: `0x0800` 3D point layout, `0x8000` hole (shapes), `0x0200`/`0x0400` new/modified (as V7), `0x0080` hidden; see [Property word](#property-word-0x28) | oracle: Z vs 2D geometry; hole ring in a cell; `0x0080`: every hidden TreeText tag (13,808), no other record of the public or private corpus |
| `0x2C` | u32 | line style | oracle (4) |
| `0x30` | u32 | line weight | oracle (5) |
| `0x34` | u32 | color index (>255 = extended color) | oracle (3, 256) |
| `0x38` | 6 × i64 | element range: low corner xyz, then the extent (high − low) xyz, in UOR, for every graphic type (text, tags and cell headers included); only the model header (`Dgn~Mh` `0x90`) stores absolute low/high corners | public test_dgnv8 (ODA): every line, line string and shape, including a zero-length line stored with extent (0, 0, 0) (`tests/v8_range.rs`); Water and the MicroStation 8.11 files of the private corpus agree on every record. An absolute high corner reads as a negative extent wherever the geometry lies below zero, and MicroStation does not draw such elements |

Type-word flags (high 16 bits): `0x2000` complex header, `0x4000` complex component,
`0x1000` set on graphic elements, `0x0080`/`0x0040`/`0x0400`/`0x0800` vary by writer
(meaning unknown, kept in `ElementHeader::type_flags`). Types without this display
header: 1, 5, 8, 9, 10, 39, 57, 63, 66, 90–93, 95–99.

### Property word (`0x28`)

Counts over the graphic pages of the private corpus (567 MicroStation 8.11 files, 1,339,778
records with a display header, 267,273 of them tags) and the three public V8 files (99,446
records); own structural counts.

| Bit | Reader | Evidence |
|---|---|---|
| `0x0800` | selects the record's point layout: set = 3D (3 doubles per point, quaternion), clear = 2D (2 doubles, angle), whatever the model header's 2D flag says | set on every private record; all 1,061,848 private lines, line strings and shapes have the 3D layout, the 27,751 of the 11 models flagged 2D included; public: Water's 66,053 records (2D model) have the 3D layout and test_dgnv8's 17 records without the bit the 2D layout in a 3D model; no counter-example. The writer sets it from the model dimension |
| `0x8000` | `dgn.hole` on shapes (6) and complex shapes (14) only | graphic pages: one cell header (2) and one shape (6) in the private corpus, two of each in test_dgnv8 (oracle: the shape is a hole ring in a cell); shared cell definitions (`Dgn^Nm`): 107,730 nested cell headers and 2,835 ellipses carry it, without a filled sibling. A cell header is no hole, so the bit becomes `dgn.hole` only where a hole ring can be; elsewhere it stays in `dgn.properties` |
| `0x0080` | `Entity::visible = false` on V8 graphics (issue #25) | TreeText's 13,808 hidden tags, all with an empty range; no other record of the public or private corpus. That graphics other than tags are hidden by it rests on the external report (issue #25) only; see "Hidden graphics" |
| `0x0200`, `0x0400` | kept raw | private: 1,339,377 / 1,315,435 records; related to the type-word flags `0x0080` / `0x0040` but not one to one; meaning unknown |
| `0x1000` | kept raw; a text with it gets no range-derived length | private: 20,304 records — every shape with a fill linkage `0x0041` (20,163), 39 shapes without one and 102 texts, 101 of whose ranges are the square origin ± L (L the stored length), not the text box; public: 85,580 records and TreeText's 13,808 tags. Meaning not established |
| `0x4000` | kept raw | private 1, public 2 |
| `0x0100`, `0x2000`, low nibble | kept raw | not observed |

### Element bodies (offsets from the element start)

2D variants store 2 doubles per point and a rotation angle (radians, CCW); 3D variants
store 3 doubles and a quaternion `(w, x, y, z)`. All coordinates are UOR doubles.

| Type | Layout | Evidence |
|---|---|---|
| 3 line | `0x68` start, then end | oracle: 4 lines, 2 zero-length lines reported as POINT |
| 4, 6, 11, 21 | `0x68` u32 count, `0x6C` pad, `0x70` points | oracle; shape outlines repeat the first point; a shape may hold one vertex (see "Degenerate shapes and null tag targets") |
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
| 37 tag | `0xA0` origin (3 doubles), `0xB8` offset (3 doubles), `0xD0` u16 tag number, `0xD2` u16 value type, `0xF0`/`0xF8` text multipliers, `0x100` orientation (2D: angle in radians + three zero doubles; 3D: quaternion), `0x138` u32 value length, `0x140` value; set = dependency `0x2717`, target = dependency `0x2710`; displayed tags: see "Displayed tags" below | TreeText (13,808 tags, 2D) and private sample (418 tags, 3D) — same offsets for 2D and 3D; `0x100`: see "Displayed tags" |
| 39 tag set definition | `0x28` magic `teSt`, `0x2C` u32 (`0xF81` in every set), `0x30` u32 (varies), `0x34` u32 definitions length, `0x38` u32 same, `0x3C` definitions (V7 layout below), set name = string linkage 1 | TreeText (1 set), private sample (8 sets) |
| 94 raster frame | see [Raster attachments](#raster-attachments) | private corpus (2,029 frames) |
| 90, 91, 92, 93 raster attachment (control) | see [Raster attachments](#raster-attachments) | private corpus |
| 96 table header | `0x0C` table number (1 levels, 2 fonts, 4 level filters, …), `0x20` u32 entry count | all V8 samples |
| 95 table entry, table 1 (level) | `0x20` u32 level id (what elements store at `0x0C`), `0x24` u32 parent (`ffffffff` none), `0x28` u32 flags (raw), `0x5C` u32 level code (240-byte entries), name = string linkage 1, description = 2 | test_dgnv8 ("Default" = 64), TreeText (2 levels), private (370) |
| Level code (`0x5C`) | The number users see for a level, independent of the id: one 0 per table (on "Default"), the nonzero codes of a table are distinct (918 of 918 tables), a level name found in two or more files carries the same code in 717 of 721 cases but the same id in only 57, and code equals id in 561 of 205,849 entries. The writer gives a new level the next code above the table's highest | private structural count |
| Second level table | Most private 3D files (261/293; plans 3/274) hold a second type-96 table-1 header in a later `Dgn^Nm` page with its own type-95 entries whose level ids repeat ids of the first table (1–9 and 64). Elements carry the id only; a reader that lets the later entry win shows them on the second table's level. The writer keeps the first definition of an id, renumbers later ones above the highest id and keeps one header; since 0.3.5 the later tables' entries follow the first table's last entry on its page, and the XAttributes of the dropped headers are dropped | private structural count |
| 95 table entry, table 2 (font) | `0x28` u32 font number, `0x2C` u16 name bytes, `0x2E` UTF-16 name | test_dgnv8 ("Arial" = 1024) |
| 5 color table (level slot 1) | `0x20` u16 screen flag, `0x22` 256 RGB triples, first = color 255 (V7 order assumed), then u16 length + table file name | private sample (layout of the triples inferred from the trailing name) |
| 36 | multi-line (assumed); not decoded → Unknown. One sample only (test_dgnv8, ODA, 296 bytes, property `0x0800`): `0x7C` u16 1, `0x7E` u16 3, `0xF8` f64 1.0, `0x100` 3 doubles (0, 10000, 20000) UOR — the oracle's `LINESTRING Z (0 1 2)` (one point); a hint, not a layout | oracle shows a 1-point line string |
| 66 (`Dgn~Mh`) | model header, below | all |

### Level entry symbology

Level entries (type 95, table 1) with a 240-byte fixed part are the only layout of the
private corpus (205,849 entries); all three public V8 samples, written by two different
applications, use 104-byte entries instead. Which applications write which layout is not
known beyond that. Fields beyond id, parent, flags and code, own structural counts:

| Offset | Type | Content | Evidence |
|---|---|---|---|
| `0x2C` | u32 | reads as Unix seconds (plausible dates); not decoded | 204,834 of 204,840 dated entries are not later than the newest element of their file; `0xFFFFFFFF` in 902, 0 in 107 |
| `0x40` | i32 | presumably the level line style (weak); kept raw in `dgn.level_style_raw` | 0 (205,443), −1 (404), one other negative value (2); of the 41 graphic elements with style `0x7FFFFFFF`, 39 are on levels with 0 and 2 on that other value; no explicit style equals a nonzero value |
| `0x44` | u32 | presumably the level line weight (weak); kept raw in `dgn.level_weight_raw` | 0 or one of three small nonzero values; nonzero only where `0x48` is nonzero; of the 41 by-level weight elements 39 are on levels with 0 and 2 on a level with a nonzero value; on the levels with one of these nonzero values, 783 of 2,742 explicit weights equal it (29 %, against 3.2 % of all explicit weights) |
| `0x48` | u32 | level color index (strong) → `Layer.color` through the file palette, raw in `dgn.color_index` | every value inside the 256-entry table; 807 of the 853 by-level colored elements with a level entry are on a level with `0x48` ≠ 0 (94.6 %), against 5,627 of 205,849 entries (2.7 %) and 2,053 of 1,071,651 explicitly colored elements (0.19 %); the 19 used levels that hold only by-level colored elements all have `0x48` ≠ 0, and of the levels with `0x48` = 0 that hold elements, 3 of 44,019 hold any by-level colored element; one nonzero index is used by exactly 100 elements, all on levels with that index; a level name found in two or more files keeps its nonzero `0x48` in 29 of 40 cases |
| `0x4C` | u32 | equal to `0x48` in every entry; not read separately | 205,849 of 205,849 |
| `0x58` | u32 | 0, or one small nonzero value in 102 entries; not decoded | — |
| `0x5C` | u32 | level code (above) | — |

The reader gives every layer from such an entry `Layer.color` (palette color of `0x48`,
index 0 included: index 0 is a color like any other) and the props `dgn.color_index`,
`dgn.level_weight_raw` and `dgn.level_style_raw`; it sets no `Layer.lineweight` or
`Layer.linetype` and does not use the layer's `dgn.weight` / `dgn.style`, which the writer
interprets. Through the reader's own level mapping (a level id defined by two tables takes the
later entry's name, open question 15) 813 of the 854 by-level colored elements are on a layer
with a nonzero color index. 104-byte entries (all three public V8 samples) are unchanged: their
symbology fields are not located (`0x3C`, `0x4C` and `0x5C` are 1 on the used level and 0 on
"Default"), and their layers keep `Color::ByLayer`.

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
| `0x100` | 3D tags: quaternion, `(-1, 0, 0, 0)` when unrotated (current private corpus: 226,755 of 267,273 3D tags; the other 40,518 hold other rotations). 2D tags: the rotation angle in radians in the first double and zeros in the other three, as 2D text stores it (issue #29 as reported: 1,074 zero and 82 `[a, 0, 0, 0]` 2D tags in the reporter's files, `a` ∈ {π, π/2, …}; those files were not available; TreeText: 13,808 2D tags, all zero). cadkit up to 0.5.2 wrote the 3D form into 2D tags as well (see Mapping decisions, Tags) |
| `0x12C` / `0x130` | u32 font number / u32 justification (7 = center-middle) |
| `0x138` / `0x140` | value length / value: 8-bit text in the locale code page with a counted NUL, i32, or f64 |
| after the value | 10 bytes, then the linkages at a 2-byte boundary: set dependency (24 bytes), target dependency (56 bytes, root type 9) |

Hidden tags written by MicroStation (TreeText) leave the range empty.

### Degenerate shapes and null tag targets

Two record patterns of the private corpus (567 V8 files) that are well formed but look
broken. Own structural counts; offsets from the element start.

**One-vertex shapes.** 92 type 6 records in 28 files store a vertex count of 1; the other
1,051,503 shapes store at least 2. All 92 are alike:

| Offset | Content |
|---|---|
| `0x04` / `0x08` | u32 68 / u32 68: 0x88 bytes, no linkages |
| `0x28` | property word `0x0E00` (3D) in 91, `0x0A00` in 1 |
| `0x38` / `0x50` | range: low corner and an extent of 1 UOR in every axis (the vertex's box rounded outwards) |
| `0x68` / `0x6C` | u32 1 / 4 zero bytes |
| `0x70` | one 3D vertex, inside the range |

None is a complex component or a tag target; 61 carry a graphic group, all a nonzero
level. The decoder required 2 vertices and kept them as `Unknown` (`dgn.type_6`, warning
`dgn.decode_failed.type_6`); after 0.5.4 a V8 shape needs 1 vertex and a one-vertex shape
maps to a `Point` (`dgn.type` = 6), as a zero-length line (type 3) does. A shape without a
vertex still fails. Line strings (4) and curves (11) keep their minimum of 2, and V7 shapes
too: no one-vertex record of these occurs in the corpora. Neither reference reads such a
shape: GDAL dgnlib (V7, `dgnread.cpp`) rejects every vertex list with a count below 2, and
ezdgn requires 3 vertices for a V8 shape. A point read this way is the vertex itself, the
only geometry the record has; a closed outline of one vertex encloses nothing.

The writer writes such a `Point` (one whose `dgn.type` is 6) back as a type 6 record with
one vertex (`0x70` + one point, range of the vertex rounded outwards); every other point is
a zero-length line. Written back into itself (`clear_seed_model`), every private file the
writer accepts keeps the type histogram of its graphic pages (corpus test
`private_write_back_keeps_element_types`); writing the one-vertex shapes as lines had turned
the 92 into type 3 records.

A V7 vertex list (types 4, 6, 11, 21, 22) whose record is shorter than its declared count
keeps the points that fit, as GDAL does, and records the shortfall
(`dgn.decode_problem` "n points declared, k stored", warning `dgn.decode_failed.type_n`);
when fewer points fit than the type needs (2 for 4, 6, 11; 1 for 21, 22) the element is
`Unknown`. Before, such a record became a polyline or spline without vertices, or a point.

**Null tag targets.** Every type 37 tag of the corpus carries one target dependency
(`0x2710`, value 1, copy option 2, root type 9, 56 bytes). 267,154 roots name a shape and 8
a complex shape of the same model. The other 111 (in 21 files) store the root
`01 00 00 00 00 00 01 00` + `ff ff ff ff ff ff ff ff`: the first half as in every other
tag, the id all ones, which no element has (no graphic or control record of any model of
the file, under any width of the id). These tags are not attached to anything: the
reader keeps them standalone (`Unknown` `dgn.type_37` with the tag as its attribute) and
reports `dgn.tag_target_missing`, which is right for them. No tag has a target that the
association lookup misses (elements inside cells and complex chains, other models). The
corpus test `private_shapes_decode_and_standalone_tags_have_null_targets` checks both
counts per file.

### Raster attachments

Evidence: own hex analysis of the private corpus (567 MicroStation V8 files, 2,029 frames,
every one linking an external TIF by path; no raster bytes are embedded). Every
attachment is the same group of ten records and one XAttribute:

| Where | Records |
|---|---|
| `Dgn^C` (control), consecutive | 92, 93 (`0x1000C`), 93 (`0x1000D`), 90, then five 91: `0x20007`, `0x20008`, `0x10009` (rarely `0x1000A`), `0x1000B`, `0x1000B` |
| graphic page `$n` | 94 frame (1,024 bytes, no linkages, never a complex component) |
| `Dgn^GA/$n`, same `n` as the frame's graphic page | XAttribute kind 170 on the frame |

In most groups the ids are consecutive in the order 94, 92, 93, 93, 90, 91 × 5. Cross
links: `90.0x38` = id of the 92 and `92.0x30` = id of the 90; both carry a dependency
linkage `0x271B` (value 1, root type 3, one root) whose root is the frame id. No other
field of the group holds an id: comparing the groups that show one raster file in one
file, the records differ only in their id, time, the cross ids, the roots, the
placement fields below and the path strings.

94 frame (offsets from the element start):

| Offset | Content |
|---|---|
| `0x0C` | level (observed on several levels, mostly the default level) |
| `0x28` | property word `0x0A00` or `0x0E00` |
| `0x38` / `0x50` | range: i64 low corner and i64 extent, like every graphic; it encloses the four image corners, rounded outwards to whole UOR (every bound within 1 UOR of a corner) |
| `0x78` | 4×4 row-major pixel-to-UOR matrix: column 0 = pixel X axis `a`, column 1 = pixel Y axis `b` (one pixel long, UOR), column 2 = (0, 0, 1) in all but 10 groups, column 3 = translation = the image's lower-left corner; row 3 = (0, 0, 0, 1) |
| `0x108` / `0x110` | f64 edge lengths along the image axes in UOR: width·`\|a\|` and height·`\|b\|` (not an XY extent: they equal the range extent only for unrotated frames) |
| `0x118` / `0x120` | f64 horizontal / vertical resolution in DPI (equal in every group, mostly 200) |
| `0x70`, `0xF8`, `0x128`, `0x140`–`0x150`, `0x358`–`0x3A0` | not decoded (`0x140`/`0x148` do not follow the TIF compression or photometry); copied |

The image is `translation + s·â·ext_x + t·b̂·ext_y` for `s, t` in [0, 1], `â`, `b̂` the
unit axes; its pixel size is `ext_x / |a|` × `ext_y / |b|`. With this reading all 2,029
corpus frames lie inside their own range; reading `0x108`/`0x110` as an extent from the
translation (cadkit 0.3.0) left 1,635 of the 1,742 frames of the 2D files outside their
range or without a solvable placement.

Control records:

| Record | Content |
|---|---|
| 90 (linkages at `0x78`) | `0x38` u64 id of the 92; string linkage 3 = bare file name, 8-bit, no terminator; string linkage `0x1F` = `file://` + absolute drive path (`file://C:\…`, backslashes, never relative), stored 8-bit when ASCII and `ff fd` UTF-16 otherwise (some files store non-ASCII paths 8-bit in the locale code page); dependency `0x271B` → frame. Padding after a string may hold stale bytes |
| 91 `0x20007` (184 bytes) | `0x28` origin (x, y, z) = matrix translation, `0x40`/`0x48` edge lengths = `0x108`/`0x110`, `0x50` rotation of `a` = atan2(a.y, a.x) in [0, 2π), `0x58` skew = atan2(b) − atan2(a) − π/2 in [0, 2π), `0x60`/`0x68` DPI, `0x70` u32 `0x20000000`, `0x88` `a` (x, y, z), `0xA0` `b` (x, y, z) |
| 91 `0x20008`, `0x10009`/`0x1000A`, `0x1000B` × 2; 93 `0x1000C`, `0x1000D` | subtype at `0x20` (91) / `0x30` (93); contents not decoded and identical within a file apart from id and time; the `0x1000B` records hold short UTF-16 constants |
| 92 (144 bytes) | `0x30` u64 id of the 90; dependency `0x271B` → frame (root at `0x84`) |

XAttribute 170 payload: eight sub-records, each `u16` index (0..7), `u16` `0x570B`,
`u32` 0, `u16` data length, `u16` `0x8000`, `u32` 0, then the data; a 4-byte trailer
(`u32` 1) follows. Sub-record 3 holds the bare file name and 4 the `file://` path, both
UTF-16LE without marker and with a counted NUL; sub-record 1 holds constants. The
auxiliary record header carries reserved 0 and flags 8.

`Dgn^GA/^AH` (4 bytes) is at least the highest auxiliary page number in every file and
most often that number + 1; its exact meaning is unknown.

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
| `0x00` | u32 | type word; `0x00800000` set = 2D model. Observed: `0x00400042` (556 private files, 3D; header element 892 or 916 bytes), `0x00800042` (11 private 2D files, 724 bytes; both Safe files, 576 bytes), `0x00000042` (test_dgnv8, ODA, 3D, 616 bytes). `0x00400000` appears only on 3D MicroStation models; its meaning is unknown | Safe files 2D, test_dgnv8/private 3D |
| `0x18` | f64 | timestamp (as elements): zero in all 567 private files, a time in the 3 public ones | |
| `0x88` | f64 | model time (inferred: a plausible time in 570 of 570 files, equal to `Dgn~H` `0x130` in 529; "Seed stripping") | |
| `0x48`, `0x4C` | u32 | unit flags (`0x11` in all samples) | |
| `0x50`, `0x58` | f64 | master unit numerator / denominator (units per meter; m = 1/1) | m in all samples |
| `0x60`, `0x68` | f64 | sub unit numerator / denominator (mm = 1000/1) | test_dgnv8 (mm), Safe (m = 1/1) |
| `0x90` | 6 × i64 | model extents (UOR) | |
| `0xC8` | 3 × f64 | global origin (UOR; 0 in all samples) | |
| `0xE0` | f64 | UOR per master unit (10000 / 10) | oracle coordinates |
| `0xE8`… | f64 | further unit data (1, 1, 1e7, …; not decoded) | |

**Stream prefix.** The bytes before the element are mostly zero. Observed relation, not a specified meaning: the first dword equals the number of
graphic and control pages (`Dgn^G/$n`, `Dgn^C/$n`) whose record count is not 0 in 552 of
567 private files and in all 3 public V8 files (test_dgnv8 1, TreeText 2, Water 68); the
other 15 private files have empty graphic pages between filled ones and a value between
that count and the number of graphic and control pages. No observed file lies outside that
range; counting auxiliary pages too breaks the relation (260 of 567). The dwords at `+4` and
`+8` are non-zero in every private file and in TreeText; their meaning is unknown (open
question 14). The writer sets the first dword to the number of graphic and control pages it
stores that hold records and keeps every other prefix byte (0.5.3; before, it copied
the seed's value, so 0.5.2 rewrites of TreeText and Water into themselves lay outside the
observed range and `check_structure` reports them with `dgn.model_page_counter`).

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
  This is a loss, since the element is then drawn in its level's color: every such
  element or tag is counted in the aggregated warning `dgn.color_extended_unresolved`.
  The by-level sentinels (color and weight `0xFFFFFFFF`, style `0x7FFFFFFF`) → `ByLayer`, no
  millimetre weight and no linetype, the numbers kept in `dgn.color_index` / `dgn.weight` /
  `dgn.style` (the writer writes them back as stored, see "Level sentinels"); `0xFFFFFFFE`
  (by cell) → `ByBlock` (not observed). The layer of a 240-byte level entry carries the
  level color ([Level entry symbology](#level-entry-symbology)), so a renderer that resolves
  `ByLayer` through the layer shows by-level elements in their level color.
- **Weights**: weight `w` → `Lineweight::Millimeters(0.1 · (1 + 2w))` (a weight-`w` line is
  `1 + 2w` pixels wide; 0.1 mm per pixel assumed); raw value in `dgn.weight`.
- **Styles**: 0–7 → `DGN_SOLID`, `DGN_DOTTED`, `DGN_MEDIUM_DASH`, `DGN_LONG_DASH`,
  `DGN_DOT_DASH`, `DGN_SHORT_DASH`, `DGN_DASH_DOUBLE_DOT`, `DGN_LONG_DASH_SHORT_DASH`;
  larger values `DGN_STYLE_n` (custom styles, no pattern). The file stores only the style
  number. Styles 1–7 are cosmetic: their dashes have a fixed size on screen or paper,
  whatever the drawing units are (issue #34: a sheet modelled in metres, about 0.5 units
  wide, with frames in style 7). Their linetype entries therefore carry a nominal pattern
  in paper millimetres, converted to drawing units with the document's `meters_per_unit`
  (1 mm = 0.001 / `meters_per_unit` units), so `Linetype::pattern` stays in drawing units
  like every other linetype; a file in metres gets `DGN_MEDIUM_DASH` = `[0.003, -0.0015]`.
  Without a known unit (V7 master label not recognised, V8 model header without a unit
  definition) the pattern is empty and the name and `dgn.style` identify the style. The
  millimetre lengths are cadkit's own choice, not file data (dash, gap, …; 0 = dot):

  | Style | Name | Paper pattern (mm) |
  |---|---|---|
  | 1 | `DGN_DOTTED` | 0, −1.5 |
  | 2 | `DGN_MEDIUM_DASH` | 3, −1.5 |
  | 3 | `DGN_LONG_DASH` | 6, −1.5 |
  | 4 | `DGN_DOT_DASH` | 4.5, −1.5, 0, −1.5 |
  | 5 | `DGN_SHORT_DASH` | 1.5, −1.5 |
  | 6 | `DGN_DASH_DOUBLE_DOT` | 4.5, −1.5, 0, −1.5, 0, −1.5 |
  | 7 | `DGN_LONG_DASH_SHORT_DASH` | 6, −1.5, 1.5, −1.5 |
- **Text**: the stored origin is the **lower-left corner** of the text box for every
  justification. Evidence: GDAL dgnlib documents it ("Bottom left corner of text") and its
  writer computes text bounds from it; smalltest's center-center text has origin x = range
  left edge; test_dgnv8's left-top texts have range low = origin. The justification code
  becomes `halign`/`valign` (0–2 left, 3–5 left margin → left, 6–8 center, 9–11 right
  margin → right, 12–14 right; top/center/bottom → `Top`/`Middle`/`Baseline`, the bottom
  of the DGN text box being the baseline), and `position` is moved from the origin to the
  justification point: `origin + x̂·(fx·L) + ŷ·(fy·H)` with `fx, fy ∈ {0, ½, 1}`,
  `H` = text height and `L` = text length — V8 `0x80` when non-zero (MicroStation writes
  it); else the range width for unrotated text whose range starts at the origin (within
  1 UOR) — V7 text, and V8 text that is not empty and has property bit `0x1000` clear
  (`dgn.text_length_source = "range"`); otherwise `characters × character width` (GDAL's
  estimate; `dgn.text_length_estimated = true`).
  `end_point = None`. The stored origin is kept in `dgn.origin`, the length in
  `dgn.text_length`, the code in `dgn.justification`. Width factor = width / height.
  Style = font name (font table) or `DGN_FONT_n`; one `TextStyle` per font. Checks:
  smalltest's center-center anchor equals the range center within 1e-6; the oracle texts
  (all left-top) keep their CSV point in `dgn.origin` with the anchor one height above it
  along the rotated Y axis; all 12,623 TreeText texts are left-bottom (anchor = origin,
  measured length present).

  V8 text length evidence (own structural counts). Private corpus, 906 texts: 405 store
  `0x80`; 303 of them are unrotated, have `0x1000` clear and a range starting at the origin,
  and their range width equals the stored length within 1e-3 relative (283 within 1e-4; the
  range is rounded outwards to whole UOR); the other 102 carry `0x1000` and 101 of their
  ranges are the square origin ± L. Against the stored lengths the estimate is too long:
  L / estimate is 0.6–0.9 (113, 97, 57 and 36 texts per tenth). The 501 texts that store 0
  are all empty (no characters) with a 1 UOR wide range: they keep the estimate, 0, as the
  range rule skips empty text. So no private text changes. Public: TreeText's 12,623 texts
  store `0x80` (and carry `0x1000`); test_dgnv8 (ODA) stores none: its three unrotated
  one-character texts take the range width (6,693 UOR, estimate 10,000), its rotated text
  keeps the estimate. ODA's text range is 9,922 UOR high for a text height of 10,000, so it
  is ODA's own glyph box: the rule's evidence is MicroStation's, and its value for ODA-written
  text is unverified (their anchors do not depend on it: all four are left-top). A
  range-derived length is written back as the stored `0x80` length; the anchor reads back
  unchanged (`writer::tests::text`).
- **Text nodes** → `Group { TextNode, origin }` with one `Text` per line (keeps per-line
  positions; `MText` would lose them).
- **Ellipses / arcs**: `Circle`/`Arc` when the axes are equal, else `Ellipse` (major axis =
  longer axis; parameters shifted by π/2 when the secondary axis is longer). Clockwise
  sweeps become the equivalent CCW range. Quaternions: GDAL convention (stored
  `(w,0,0,z)` with `z = sin(−θ/2)` is a CCW rotation by θ).
- **Flat ellipses / arcs** (issue #24): when the smaller semi-axis is at most
  1e-9 × the larger one (the tolerance used for equal axes) the element draws a segment,
  and `Ellipse { ratio }` would leave its documented (0, 1] range (the report saw 0 and
  6e-17). It becomes a `Line` between the extreme points the sweep reaches along the
  non-zero axis: the whole diameter for a full sweep; for a partial sweep the positions at
  both ends and ±1 wherever the swept range passes an axis end (parameter 0 / π for a flat
  secondary axis, π/2 / 3π/2 for a flat primary axis). The line starts at the extreme
  nearer the arc's start point, so an arc that passes no axis end keeps its travel
  direction (complex chain continuity). Both axes zero, or a segment of zero length →
  `Point`. The element data stays in props: `dgn.degenerate_ellipse = true`,
  `dgn.primary_axis` / `dgn.secondary_axis` and `dgn.center` (document units),
  `dgn.start_angle` / `dgn.sweep_angle` (arcs, radians as stored) and the stored
  orientation (`dgn.rotation` angle in 2D, `dgn.quaternion` `(w, x, y, z)` in 3D). Ratios
  above 1e-9 stay `Ellipse` (a consumer with a stricter lower bound, e.g. 1e-6 for some
  DXF readers, still has to handle them). The writer writes such an entity back as a
  type 3 line (or zero-length line for a point); it does not recreate the type 15/16 element
  from the props. No flat ellipse or arc occurs in the public or private corpus (0 of the
  type 15/16 elements); the tests are synthetic.
- **Curves (11)** → `Spline` with `fit_points` = stored points minus the two tangent points
  at each end (oracle: stroke starts at point 3, ends at point n−2). `dgn.curve_points`
  keeps every stored point (document units, `[x, y, z]` lists in stored order, tangent
  points included), so the element can be reproduced. The end conditions are also given
  (issue #28) as `dgn.curve_start_tangent` / `dgn.curve_end_tangent`
  (`[x, y, z]`, in the entity's coordinates; `Entity::transformed` keeps them only under a
  translation and positive uniform scale and drops them otherwise, so a rotated or mirrored
  copy, such as an exploded shared cell instance, writes free fit-point ends instead of stale
  tangents), derived as GDAL dgnlib's `DGNStrokeCurve` derives the slopes at the
  first fit point `P[2]` and the last one `P[n−3]`: with `m[k]` the unit direction of the
  chord `P[k] → P[k+1]` (zero for a zero-length chord), each component is the Akima mean
  `(m[k−1]·|m[k+1] − m[k]| + m[k]·|m[k−1] − m[k−2]|) / (|m[k+1] − m[k]| + |m[k−1] − m[k−2]|)`,
  or `(m[k−1] + m[k]) / 2` when both weights are zero. The vectors are dimensionless slopes
  along the chord length, **not normalised** (each component lies in [−1, 1]; the length
  is not 1 in general and may be 0); normalise them for a direction. GDAL measures chords
  in XY; 3D curves use 3D chords (equal for 2D files). Curves of fewer than six points
  (GDAL does not stroke them) get no tangents. `EntityKind::Spline` has no tangent fields
  in 0.5.x. The DXF writer emits them, normalised, as SPLINE groups 12/13 of fit-point
  splines; its R12 polyline output and the DGN writer (which samples splines into line
  strings) do not use them. Checked: the two test_dgnv8 curves get tangents and still meet
  the oracle's start and end points; their tangents, (0.5, −0.5) at the start and (0.5, 0.5)
  at the end, equal those of an independent port of `DGNStrokeCurve` (`tests/v8_public.rs`).
- **B-splines (27 + 21/26/28)** → `Spline`; uniform knots: clamped for open curves,
  periodic curves unwrapped into an open NURBS (poles + first `degree` poles, knots
  0..n+order) so standard evaluation traces the closed curve.
- **Cells (2)** → `Group { Cell, name, origin }`. **Shared cell definitions (34)** →
  `Block` (name from string linkage 1, base point = origin); **instances (35)** → `Insert`
  with scale = matrix column lengths, rotation/normal from the matrix (raw matrix in
  `dgn.matrix`).
- **Complex chains/shapes** → `Group { ComplexChain | ComplexShape }`; 18/19 and unknown
  headers with components → `Group { Other }`. V8 hierarchy (`tree_v8`): a component
  decrements the innermost open header's declared count (`0x68`); a non-component closes
  every open header. A complex chain or shape header (12/14)
  that arrives while a chain or shape is open closes it instead of nesting into it (issue #27),
  because a chain/shape owns plain components only: GDAL dgnlib's complex-header reader
  collects line geometry only and notes that a child "should verify complex bit set, not
  another header"; every 12/14 header of the public (6) and private (6) corpus declares
  exactly the run of plain components that follows it, and none holds another header.
  The reporter's files (14 chains in chains, up to ~14 levels inside cells) were not
  available; the staircase they describe is what an over-declared count produces
  (`Cell → chain A (declares n, stores fewer) → … → chain B`, B becoming A's component), and
  is reproduced synthetically. Such early closes (a chain or shape header that is itself a
  component closing open chains or shapes) are counted in the warning `dgn.complex_count`;
  a non-component element that closes an over-declared chain (another top-level chain, a
  line, a cell) is no early close and is not counted. Not covered: an over-declared chain
  followed by a plain component
  of the enclosing cell (it still joins the chain; nothing in the record distinguishes
  it), and B-spline curve headers (27), which may stay chain components.
- **Line strings / shapes (4, 6)** → `Polyline` (`normal = +Z`: DGN has no bulges); shapes
  are `closed` and drop the repeated first vertex. A zero-length line (3) is a `Point`
  (the oracle reports it as POINT).
- **Point strings (22)** → `Group { Other, name "point string" }` of `Point`s.
- **Tags** → `Attribute { tag: definition name, value, set: set name, position: origin +
  offset, invisible: property bit 0x0080, layer: the tag element's level when it is not
  the target's, display: size, font style, alignment and rotation of displayed tags,
  color / lineweight: the tag element's own, when they differ from the target's,
  props: symbology and flags that differ from the target's }` on the target entity (looked up by element
  id, also inside groups). The display rotation comes from `0x100`: an angle in 2D tag
  elements, a quaternion in 3D ones (issue #29 as reported; before, a 2D `[π, 0, 0, 0]`
  was normalised as a quaternion and read as unrotated). cadkit up to 0.5.2 wrote
  quaternions into 2D tags too (`(-1, 0, 0, 0)` unrotated, else `(cos θ/2, 0, 0, −sin θ/2)`),
  so a 2D tag whose four doubles are exactly `(-1, 0, 0, 0)`, or `(w, 0, 0, z)` with `z ≠ 0`
  and `|w² + z² − 1| < 1e-9`, is read as a quaternion (`TagData::display_rotation`); an
  angle never has a nonzero fourth double, so the only ambiguity is a 2D tag stored at
  exactly −1 rad, which reads as unrotated. A non-finite stored angle reads as 0, as for
  text, instead of failing a later export. `color` is resolved like an
  entity color (file color table, else the default palette; `ByBlock` for
  `0xFFFFFFFE`) and `lineweight` like an entity lineweight; both stay `None` when the
  tag's `dgn.color_index` / `dgn.weight` equal the target's (then those props are dropped
  too), so `None` means "the owner's" (issue #30; private corpus: 20,624 of
  267,273 tags have their own color index, 33,797 their own weight). Tags whose target is missing (and V7 tags) stay as
  `Unknown { "dgn.type_37" }` entities carrying the attribute (`dgn.tag_target_missing`).
- **Hidden graphics** (issue #25): a V8 element with a display header and
  property bit `0x0080` maps with `Entity::visible = false` (the raw word stays in
  `dgn.properties`). The bit marks hidden tags in TreeText (13,808 of 13,808), and the
  report states that other DGN readers do not draw graphics carrying it; no graphic
  element of the public or private corpus has it, so the change is verified on synthetic
  records only. It applies to every display-header element alike: raster frames, cell and
  complex headers (their children keep their own bits; an invisible group hides them in
  renderers that skip invisible entities) and standalone tags. V7 is unchanged: GDAL
  dgnlib's property flags (`DGNPF_*`) give `0x0080` no meaning. The writer sets the bit
  from `visible` only (`dgn.properties` contributes its `0x0600` bits), so it is not applied
  twice, and a hidden owner no longer hides the displayed tags written for it (their bit
  comes from `Attribute::invisible`). Limitation: the neutral document walk
  (`Document::walk`, which SVG output uses) skips an invisible entity together with its
  attributes, so the displayed tags of a hidden owner are not drawn in SVG although the
  file shows them; the DGN writer keeps them. Neither the public nor the private corpus
  has such a tag (the only hidden graphics are TreeText's tags themselves).
- **Raster frames (94)** → `Image`: `position` = matrix translation (lower-left image
  corner), `u_vector = â·ext_x`, `v_vector = b̂·ext_y` with `â`, `b̂` the unit matrix X / Y
  columns and `ext` the edge lengths at `0x108`/`0x110` (see
  [Raster attachments](#raster-attachments)); `size_px = (round(ext_x/|a|),
  round(ext_y/|b|))`. Rotated and sheared frames need no solving. The element range is
  only a consistency check: when the four corners do not lie inside it within 4 UOR the
  rectangle is rebuilt from the range — along the matrix axes when that is solvable, else
  axis-aligned — and `size_px` is left `None` (`dgn.raster_outside_range`); without a
  usable transform the range is used (`dgn.raster_range_placement`). Props: `dgn.range`
  (element range in document units), `dgn.raster_matrix`, `dgn.raster_dpi` (`0x118`;
  `dgn.raster_dpi_y` only when `0x120` differs), `dgn.raster_rotation` (angle of the image
  X axis) and `dgn.raster_skew` (angle of the Y axis from the perpendicular), both in
  radians in (−π, π] in the XY plane, `dgn.raster_file` (90 string 3), `dgn.raster_path`
  (90 string `0x1F`, the `file://` URL; absent when empty). Path = attachment full path,
  else file name.
- **Everything else** → `Unknown { "dgn.type_N" }` with `dgn.*` props; `keep_raw` stores the
  exact element bytes. Undecodable bodies keep their header/symbology and get
  `dgn.decode_problem`.
- Warnings use stable codes: `dgn.page_records`, `dgn.page_count`, `dgn.zlib`,
  `dgn.stream_unreadable`, `dgn.model_header`, `dgn.model_index`, `dgn.v7_truncated`,
  `dgn.v7_units`, `dgn.unknown_element.type_N`, `dgn.decode_failed.type_N`,
  `dgn.tag_target_missing`, `dgn.raster_unlinked`, `dgn.bspline_knots`, `dgn.depth_limit`,
  `dgn.complex_count`, `dgn.color_extended_unresolved`
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
+ 8-bit text (V8 text, test_dgnv8 "myTéxt"); otherwise 8-bit.

Precedence per string kind (`text.rs`; issue #26):

| String kind | Rules, first match wins |
|---|---|
| V8 string linkages `0x56D2` (level, model, cell, shared cell and tag set names, descriptions, unit labels, raster file names and paths) | `ff fd` → UTF-16LE; `ff fe 01 00` → marked 8-bit text; `ff fe` + anything else → UTF-16LE after a byte-order mark; else 8-bit. UTF-16 stops at the first NUL unit, an odd trailing byte is ignored |
| V8 text payloads (17) and text tag values (37) | `ff fd` → UTF-16LE; `ff fe 01 00` → marked 8-bit; else 8-bit (no byte-order-mark rule: none was seen) |
| UTF-16 fields without a marker: model index names and descriptions, font table names | UTF-16LE; a leading `ff fe` byte-order mark is dropped |
| Tag set definition strings (type 39 / V7 66 body) | `ff fd` → UTF-16LE to the NUL unit; else NUL-terminated 8-bit (unchanged) |
| V7 text | `ff fd` → 16-bit units as GDAL dgnlib reads them; else 8-bit (unchanged) |

`ff fe 01 00` keeps its meaning in string linkages too: ezdgn documents it on MicroStation
CONNECT unit labels and cell names. Read as UTF-16 after a byte-order mark it would start
with U+0001, a control character no name holds, so the rule cannot misread a real name.
Evidence for the byte-order-mark form comes from the issue only: names came out as
`"ÿþ"` + the low bytes up to the first ASCII character — the 8-bit path stops at the first
NUL byte, and the level-name deduplication appends ` (id)` to names that collapse to the
same text, which matches `"ÿþ- (7)"`. Neither the public nor the private corpus stores one: 0 of 542,525 private
string linkages start with `ff fe` (4,277 `ff fd`, the rest plain), and no layer or model
name starts with `ÿþ` or U+FEFF, before or after the change. The writer stores new names
as `ff fd` UTF-16 (raster paths 8-bit when ASCII) and keeps seed dictionary records byte for
byte, so a byte-order-marked name read back from either decodes to the same text. 8-bit text is decoded with
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
| private corpus rasters | 567 files, 2,029 frames: every frame placed from its matrix inside its own range with a pixel size, no `dgn.raster_outside_range` / `dgn.raster_range_placement` (`tests/private_corpus.rs`); a corpus seed takes a new attachment whose records have the corpus record signature and which reads back at the source attachment's placement |
| private level colors and text lengths | every layer from a 240-byte entry has its palette color and raw fields (205,849 layers, 5,627 with a nonzero index); 854 by-level colored elements stay `ByLayer` with the sentinel; 303 stored text lengths equal the range width within 1e-3; no private text takes a range-derived length; written back into themselves, all 567 files give the same bytes with and without the layer colors (528 written, 39 fail alike) — `tests/private_corpus.rs` |
| robustness | truncations and bit flips of every sample (`tests/robustness.rs`); mutations of the inflated pages repacked into new compound files (`src/fuzz_tests.rs`); V7 body stamping |

## Open questions

An external pass with the acceptance kit settles several of these for the application
used; [ACCEPTANCE.md](ACCEPTANCE.md#7-open-questions-this-pass-can-settle) lists which and
the observation each needs.

1. **Level flags and level symbology.** Flags (`0x28` of level entries) are not decoded;
   layers are reported visible, unfrozen, unlocked, with the raw value in
   `dgn.level_flags`. Observed: test_dgnv8 "Default" (used) `0x02600002`; TreeText and Water
   "Default" (unused) `0x026F0002` and the used level `0x02670002`; private corpus (202,506
   distinct level ids): `0x026F0006` (202,185), `0x02670006` (315), `0x02600006` (4),
   `0x03600006` (2), and bit `0x00080000` is set on 99.75 % of the used and 99.90 % of the
   unused levels — no bit separates an observable state. Symbology: the color index of
   240-byte entries is located (`0x48`, [Level entry symbology](#level-entry-symbology));
   weight (`0x44`) and style (`0x40`) are weak and kept raw; the fields of 104-byte entries
   (the layout of the three public samples) are not located. Settled by: a file saved with a
   level switched off, frozen or locked next to the same file before; a file with a level
   whose weight and style are set to known nonzero values and elements that use them by
   level; a file with 104-byte level entries and a colored level.
2. **B-spline "closed"**: both test_dgnv8 curves have `0x6C = 0x22` (bit `0x80` clear) and
   `0x6D = 0x01`, yet the oracle strokes them as periodic. cadkit treats bit 0 of `0x6D`
   as "closed" in V8. The private corpus has no B-spline. Settled by: a sample with an open
   non-uniform curve (and a closed one).
3. **Extended colors** (index > 255): the RGB is not located. One element in all corpora
   (a test_dgnv8 text, index 256); the oracle gives it two different RGBs in two exports and
   neither byte triple occurs in any raw or inflated stream of the file; the private corpus
   has none. Issue #33 reports that they are common in production files (20 of 45 files,
   11,219 elements, 7,280 of the 33,594 elements of one file; example raw value `0x4A03`),
   so the reader now warns (`dgn.color_extended_unresolved`). Settled by: a file with
   extended colors whose RGB values are known.
4. **Sheet models**: no public sample with a sheet model was found, and every sample has
   one design model (567 private files: one model each, model index flags 0 in all 570 V8
   files), so no field can be tied to design/sheet. Every model is reported as
   `ModelKind::Model` (`dgn.model_index_flags`, `dgn.model_type_word` kept); 2D/3D comes
   from bit `0x00800000` of the model header type word (observed values under "Model
   header"). Settled by: a file with a sheet model next to a design model.
5. **Storage vs master unit**: `0xE0` is used as UOR per master (matches the oracle);
   `0xE8`/`0xF0` might be a storage unit definition — irrelevant while both are 1/1, as in
   all 570 V8 files. Settled by: a file whose storage unit differs from its master unit.
6. **Type 33 dimensions, 36 multi-lines, 26/28 V8 knots and weights, 18/19 bodies**: left
   Unknown / unverified. The private graphic pages hold only types 2, 3, 4, 6, 14, 15, 17, 37
   and 94, all decoded. Type 36 has one degenerate public sample (one point, see the element
   table). Settled by: files with dimensions, multi-lines, non-uniform or rational B-splines
   and 18/19 headers.
7. **V7 tag targets**: not attached. GDAL dgnlib reads only the set number (68), tag number
   (72), type (74), length (150) and value (154) of a tag element and has no reader or
   writer code for the tag-to-element association; no public V7 sample contains tags and the
   private corpus has no V7 file. Settled by: a V7 file with tags attached to elements.
8. **Hidden flag** `0x0080`: on tags inferred from one file (TreeText, 13,808 tags with an
   empty range); on other graphics taken from issue #25 (reported externally with counts; no
   record outside TreeText's tags carries the bit in the public or private corpus). V7's bit
   `0x0080` stays uninterpreted. Settled by: a file with a hidden graphic element that is not
   a tag.
9. **Text length estimate**: V7 rotated text and V8 text without a stored length and without
   a usable range (rotated, bit `0x1000`, range not at the origin) use `characters ×
   character width`, which is exact only for fixed-pitch fonts (MicroStation's stored
   lengths are 0.6–0.9 of it). No such non-empty V8 text occurs in the corpora. The
   range rule rests on MicroStation texts; for ODA-written text (test_dgnv8) the range is a
   glyph box and its width is not verified as the length. Settled by: a rotated V8 text
   without a stored length, and an ODA-written text whose MicroStation re-save stores a
   length.
10. **Raster extent** (resolved in 0.3.1): `0x108`/`0x110` hold the edge lengths along
    the image axes (width·|a|, height·|b|). 0.3.0 read them as an XY extent from the
    translation, which matched only exactly unrotated frames; 1,635 of the 1,742 frames
    of the private 2D files then fell outside their range or could not be solved. No
    public sample contains a raster frame.
11. **Complex counts**: the over-declared chain/shape counts behind issue #27 were not
    observed (every corpus chain/shape count is exact); the closing rule is an inference
    from the report and GDAL's reader, see "Mapping decisions".
12. **Byte-order-marked names** (issue #26): no corpus file stores one; the precedence in
    "Strings" rests on the report.
13. **Line style table**: the type-96 table holding custom line styles has not been
    identified (table numbers seen: private 1–8 in every file, 19 in 536, 18 in 2; public
    test_dgnv8 {1, 2, 4}, TreeText {1, 3–7, 19}, Water {1, 4–7}), and no sample uses a custom
    style: private graphics store style 0 or `0x7FFFFFFF` (42 records), the public ones 0
    or 4. The writer therefore cannot check a `dgn.style` number against the seed; a sample
    with a custom line style would settle it.
14. **Model header prefix dwords** `+4` and `+8`: non-zero in every private file and in
    TreeText, meaning unknown; the writer keeps the seed's values. The first dword's
    relation to the page count is an observation (see "Model header").
15. **Repeated level ids**: 3,343 private level entries repeat the id of an entry of an
    earlier level table (see "Second level table"), and 1,885 of them have a different
    `0x48` color. The reader names
    an element's layer after the later entry (and gives it that entry's color), the writer
    keeps the first definition; which entry MicroStation shows is not known. Settled by: a
    file where the two definitions differ, with MicroStation's display of an element on that
    id.
16. **Property bits** `0x0200`, `0x0400`, `0x1000`, `0x4000`: kept raw, meaning unknown
    (counts under "Property word"). Settled by: files saved before and after the
    corresponding edit (new / modified element, fill, …).
17. **File header and model header fields that differ between writers**: nine `Dgn~H`
    dwords (`0x110`, `0x118`, `0x138`, `0x13C`, `0x140`, `0x308`, `0x310`, `0x314`,
    `0x388`), the model header prefix dwords `+4` / `+8` (open question 14), the model header
    fields `u32 0x20`, `u32 0x44`, `f64 0xF8`, `0x120`, `0x140`, `0x160`, `0x1C8`, `0x1D8`
    and type word bit `0x00400000` hold different values in test_dgnv8, TreeText and Water
    (table under "Built-in blank seed"). `0x138` and `0x140` take 72 and 52 values in the
    private corpus and match none of the record and page counts tried (tables, levels, views,
    tag sets, dictionary and auxiliary pages and their counters). The writer copies them from
    the seed; a seed cadkit would make itself needs their meaning first. Settled by: files
    saved by one application with one setting changed at a time, or a public description of
    the fields.

## Seed-based V8 writer

`write_v8(Document, seed, WriteOptions)` constructs new model graphics in a
single-model seed from one model of the document (see "Source model" below). Known master
units must match the seed, and so must the dimension unless
`WriteOptions::dimension_policy` allows the difference. A 2D plan reaches a 3D seed through
`V8Seed::prepare_document` (`DimensionPolicy::Promote2dTo3d`, the default; acceptance kit
file 09) or through `dimension_policy`; `write_v8` never drops a nonzero Z unless
`DimensionPolicy::ProjectToSeed` asks for it. Nonempty seeds require `clear_seed_model=true` explicitly.
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
construction. Existing streams and directory metadata (CLSIDs, state bits, times) are copied unless
replaced or explicitly removed; the directory itself is rebuilt (slot order, sibling trees
and unused slots, "Compound file"). Fresh entry times derive from the seed root, making
repeated output deterministic. Model graphic/control/auxiliary pages are replaced;
named tables are retained and extended. Unknown seed streams are preserved, not
interpreted or certified as free of references to cleared graphics.

**Dictionary pages** (0.3.5, `writer/named.rs`). The seed's `Dgn^Nm` layout is kept
(see "Dictionary pages" above): every seed page keeps its number and its records their
order, and a page whose records did not change keeps its stream byte for byte, empty
pages included. A changed record is updated where it is (the level table header's count,
an extended tag set); a new level follows the last entry of the level table, on its page,
with the flags of the seed's entries and the next free level code; new tables and tag
sets share one new page after the seed's last page (a seed without a level table gets
the table there, before the new tag sets). Rewritten pages keep their own format version
and store page number and population 0; new pages take the first seed page's version. A
page is never split at a fixed size: the dictionary records together are bounded only by
`Limits::max_decompressed_bytes`, beyond which writing fails with `LimitExceeded`. A seed
whose writer split a complex element (cadkit before 0.3.5 did, at 256 KiB) is repaired:
components at the start of a page rejoin the page of the element before them, and level
entries after the level table header join that table. XAttributes (`Dgn^NmA/$n`) of moved
records move to the page numbered like their element's new page, those of dropped records
are dropped, and only changed auxiliary pages are rewritten (page number and population
0); `Dgn^NmA/^AH` is kept. Before 0.3.5 the writer concatenated every dictionary record
into new 256 KiB pages numbered from 1 with the page number and record count in the page
header, so a 330-entry level table was split over two pages and MicroStation did not load
it as one. Graphic pages are unchanged: new graphics still go to 256 KiB pages.

Streams are matched for replacement by their CFB names joined with `/`. The `cfb` crate
builds entry paths with the platform separator (`\` on Windows), so its path strings are
not used as keys; before this rule, Windows output kept every nested seed stream
unchanged and dropped new pages whose numbers collided with seed pages. A numbered
(`$n`) page the writer would replace but the reader could not decode is an error, since
its content could be neither checked for emptiness nor carried into the named tables.

| Written structure | Layout evidence |
|---|---|
| File header | Existing reader and own corpus analysis; zlib payload after `0x14`, expected 1,576 bytes; `0x128` high-water element ID updated |
| Model header | Existing decoded header bytes, including original stream prefix; model extent fields at `0x90` updated; prefix dword 0 = graphic and control pages holding records (observed relation, "Model header") |
| Container | Directory in the writer's slot order, top-down red-black sibling trees, unused slots in the MS-CFB 2.6.3 free form ("Compound file") |
| Element pages | Existing page decoder, zero delimiter per record, bounded batches, zlib encoding |
| Element identity | 64-bit ID at `0x10`, allocated above both observed records and header high-water mark |
| Display/geometry | Existing V8 decoder layouts; finite UOR conversion, 64-bit range bounds |
| Levels, tag sets, tags | Existing decoded tables/linkages, seed templates when available, dependency target IDs |
| Dictionary pages | Seed page layout kept (private corpus structural counts, "Dictionary pages"); field test of a 0.3.4 output against the same file with the seed's dictionary pages |

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
hole-marked shape components. Polygon rings and mesh faces inherit the level, colour,
line style, lineweight, visibility and `dgn.color_index` / `dgn.style` / `dgn.weight` of
their entity (before 0.5.3 mesh faces took only level and colour). Polygon rings must lie within
`WriteOptions::planarity_tolerance` (default 0.01 source units, 1 cm in metres) of the
exterior ring's plane; 1e-6 rejected every polygon with a hole in 0.1 mm-rounded
CityGML sources. Shared-cell instances, edited raster attachments, splines,
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

**New raster attachments** (0.3.1). An `Image` without an `id`, with `path` and
`size_px`, becomes a new attachment cloned from the first complete attachment group of
the seed (`seed_has_raster_template` tells whether there is one). Since 0.3.3 a seed
without one gets a built-in group (`writer/raster_builtin.rs`): the bytes equal in all
2,316 complete groups of the private corpus, and for the undecoded display fields that
differ between files (frame `0x29`, `0x34`, `0x70`, `0xFE`, `0x12A`, `0x144`–`0x14B`;
`0x20008` record `0x2A`–`0x3C`; 90 `0x28`; control levels) one combination observed
together: bitonal display with foreground `FFFFFF` and background `000000`, frame
colour 0 and weight 1, 200 DPI, control records on level 64. Its XAttribute pages take
the format version of the new graphic pages (every observed file whose graphic pages
are all version 3 has version 3 auxiliary pages). The template is captured from the parsed seed before `clear_seed_model` drops
its records. Each image gets ten fresh ids above every seed id (94, 92, 93, 93, 90,
91 × 5) and the group's records are copied with only these fields rewritten: every
record's id and time; 90: the 92 id, string 3 = bare file name (`dgn.raster_file`, else
the last component of `path`), string `0x1F` = the full path: for an absolute `path` the
`file://` URL (`path` as given when it already starts with `file://`, else `file://` +
`path`); since 0.3.4 a relative `path` naming a file beside the DGN stores no machine
location — a bare file name stores an empty string `0x1F` (and an empty XAttribute
sub-record 4, the NUL unit only), `.\name` / `./name` is stored as given. Both strings are
8-bit when ASCII and `ff fd` UTF-16 otherwise, and the frame root; 92: the 90 id and the
frame root; 91 `0x20007`: origin, edge lengths, rotation, skew, DPI and both axes; 94:
level (from `layer`), hidden bit `0x80` (from `visible`), range, matrix (`a = u/W`,
`b = v/H`, column 2 = unit normal of the image plane), edge lengths `|u|`, `|v|` and DPI
(`dgn.raster_dpi` / `dgn.raster_dpi_y`, else the template's); XAttribute 170:
sub-records 3 and 4 and their lengths. Everything else (symbology, the other 91/93
records, the undecoded frame fields, the XAttribute constants) is the template's. Control
records follow the retained seed controls in `Dgn^C`; each XAttribute goes to the
`Dgn^GA/$n` page numbered like its frame's graphic page, and `Dgn^GA/^AH` is set to the
highest such page + 1. The writer is the inverse of the reader: an attachment reads back
with the same `position`, `u_vector`, `v_vector` (1e-6 relative), `size_px`, the
`file://` form of an absolute path (a relative one reads back as written, a bare name
through the file name) and no raster warning. Every observed attachment stores an
absolute `file://` path; whether MicroStation resolves an empty or `.\name` full path
from the DGN's folder is not yet confirmed. Mixing new attachments with preserved seed
attachments (images that keep their id) in one write, new attachments inside groups,
relative paths into other folders and strings longer than a 512-byte string linkage are
rejected.

`preserve_seed_rasters` (default true) retains unchanged attachments from the same
seed. All image entities must have unique original IDs and unchanged geometry,
paths, display properties and native props. Frame records (94), raster control
records (90..93), and their auxiliary payloads retain their bytes and IDs; other
graphics and attached tags are regenerated. `preserve_seed_controls` (default true)
keeps every other seed control record (coordinate system, model settings and the like)
and its auxiliary data byte for byte, unless a dependency linkage of the record names a
seed graphic as a root (up to this change: unless any byte window of the record equalled a
seed graphic id; see "Kept control records" under "Seed stripping");
the control auxiliary page counter is kept while all of its pages are. Incomplete
auxiliary pages, mixing preserved and new attachments, partial deletion of preserved
seed attachments, or edited preserved attachment settings are rejected. An output is
still dependent on its external raster file; no raster bytes are embedded or
fabricated. New attachments clone a seed attachment group (`seed_has_raster_template`)
or fall back to the built-in template group added in 0.3.3 when the seed has none,
rewriting only the documented placement, path, and resolution fields rather than
encoding arbitrary raster display settings from scratch.

Level names, tag sets and attached text/integer/double values are writable.
Unnamed tag sets remain unnamed rather than being silently renamed to `CADKIT`.
Standalone type-37 tags are written without an invented owner; their values and
positions are retained without creating a dangling dependency. An attribute with
`display` is written as a displayed tag in the layout of "Displayed tags": origin at
the owner's first vertex, offset to `position`, size, font (`display.style` resolved
by name like text styles, or `dgn.font_number`), justification and orientation at `0x100`
(a 3D seed: quaternion; a 2D seed: the angle in the first double, zeros after it, as 2D
text — issue #29; before, 2D seeds got a quaternion that the reader then lost); its range
uses `dgn.text_length` when given, else characters × width. `Attribute::layer` places the
tag element on its own level, and the attribute's `dgn.color_index`, `dgn.weight`,
`dgn.style`, `dgn.graphic_group`, `dgn.properties` (`0x0600` bits) and `dgn.type_flags`
(`0x00C0` bits) props override the owner's. Without a `dgn.color_index` / `dgn.weight` of
its own, a typed `Attribute::color` / `lineweight` is used instead of the owner's and is
resolved exactly like an entity's (color matched against the seed palette, failing when it
is absent; a millimetre lineweight needs a native index); without either, the owner's
symbology is inherited as before. The tag's hidden bit `0x0080` comes from
`Attribute::invisible` only, no longer from a hidden owner. Owner dependencies use MicroStation's
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

ACI/RGB colors are matched exactly against the seed's palette (`palette::exact_index`, the
lowest matching index; `V8Seed::prepare_document` maps off-palette colors with
`palette::nearest_index`, squared RGB distance, lowest index on a tie). An unavailable
color fails unless the caller explicitly provides `dgn.color_index`. ByLayer
color uses the layer's `dgn.color_index` (0–255), else a supplied neutral layer color
(exact palette match), or seed palette index zero when no explicit color exists. This
materializes the color, not a live inheritance rule.
ByBlock requires prior resolution. Native lineweight indexes must be in `0..31`;
an entity's explicit `dgn.weight` takes precedence over the neutral layer's
`dgn.weight`. A millimetre-only weight has no universal DGN index mapping and
requires explicit caller conversion.

**Level sentinels** (0.5.3). Elements store color `0xFFFFFFFF` and weight
`0xFFFFFFFF` (with style `0x7FFFFFFF`) when they take them from their level: 854 graphic
records of 54 private files store the color value (text, shapes, a cell and raster
frames), 42 records of 2 files the weight and style values; no public sample has them. The
reader maps the color to `Color::ByLayer` and the weight to no millimetre value, keeping
the numbers in `dgn.color_index` / `dgn.weight` / `dgn.style`. An element's (or tag's)
`dgn.color_index` or `dgn.weight` of `0xFFFFFFFF` is written back as stored, and
`prepare_document` keeps it; a layer's `dgn.weight` of `0xFFFFFFFF` is still rejected.
Before, the writer rejected such a document ("extended DGN color table entries"): 51 of the
54 private files could not be written back into themselves; now 52 of 54 can (the other
two fail on unrelated content: an unknown element and a non-zero Z in a 2D model).

**Level colors** (0.5.3). Layers read from 240-byte level entries carry the level color
(`Layer.color` and `dgn.color_index`, [Level entry symbology](#level-entry-symbology)). A
`ByLayer` entity without its own `dgn.color_index` is written with the layer's
`dgn.color_index` first, as it takes the layer's `dgn.weight`; before, only the layer's
color was matched exactly against the seed palette, which fails for a seed without that RGB
and picks the lowest of repeated palette entries. Elements read from a file carry their own
index (the sentinel included), so a document read and written back gives the same bytes as
before: all 570 private and public V8 files written into themselves produced unchanged output
(528 private and 2 public written, 40 failing as before), and `prepare_document` outputs
changed only for test_dgnv8 (its range-derived text lengths, "Mapping decisions"). A new `ByLayer`
entity placed on a colored level is now written with that level's index instead of 0 (in a
sweep adding one line to the first level of each private file, 121 of 528 outputs changed).
The writer does not write `Layer.color` into level entries: a new level copies the seed's
first level entry, its `0x40`–`0x4C` fields included, and reads back with that entry's
color.

**Fonts.** Nonzero font indexes must be in the seed's font table or actually referenced by
seed Text/TextNode records; font zero is the default when a text names neither a style nor
a number (before 0.3.0 the first table entry by name was taken, an arbitrary seed font). A
text style names a font by its exact table name. When the table stores that name with two
different numbers the style is ambiguous: the write fails ("text font style is ambiguous in
seed") unless `dgn.font_number` names one of the two (before 0.5.3 the
last entry won silently). A named style must agree with its numeric font index.
`V8Seed::font_entries` lists the table as stored and `V8Seed::font_number` resolves a name
(exact first, then trimmed and ASCII case-insensitive; absent and ambiguous names are
distinct `Unsupported` errors) for a host that matches font names loosely. The private
corpus has 1,701 font entries and no ambiguous name; `prepare_document` drops an
ambiguous style as it drops an unknown one.

**Line styles.** `dgn.style` must be an integer (any other value fails); every `u32` is
written as given at `0x2C`. 0–7 are the standard styles; other numbers (custom styles,
`0x7FFFFFFF`) are not checked against the seed, whose line style table has not been
located (open question 13); the reader reports values from `0x7FFFFF00` up without a
linetype. Existing seed level settings are retained;
writing arbitrary neutral level flags, custom fonts/linetypes, CAD props and
layout/application semantics is not implemented. This is new geometry export,
not general lossless Document-to-DGN roundtripping.

**Record times.** New records (graphics, tags, levels, tag sets, raster records) take
`WriteOptions::timestamp_ms`; when it is 0 they take `V8Seed::timestamp_ms` (`Dgn~H`
`0x130`). Unchanged seed records keep their bytes and time. When both are 0 the records are
written with time 0, which `check_structure` reports as `dgn.record_time`; every graphic
and control record of the public and private corpus has a positive time.

### Source model, dimension and text length

Every DXF and DWG document holds model space and its paper layouts (often empty), and a
DGN document one model per design or sheet model, while the writer fills the one model of
its seed. Since the release after 0.5.4 (`WriteOptions` additions, all off by default
except the model choice):

| Option | Behaviour |
|---|---|
| `model_index: Option<usize>` | `None`: the only model, or the only `ModelKind::Model` of a document whose other models are all `ModelKind::Layout`; any other document (two design models, a sheet model beside the design model, two or more layouts only, no model) fails with "DGN writer requires exactly one source model; choose one with WriteOptions::model_index", and a document whose only model is a layout with "DGN writer accepts design models only". The DGN reader maps every model of a file, sheet models included, to `ModelKind::Model` (the model header's type word is kept in `dgn.model_type_word`), so a DGN document with several models always needs an index. `Some(i)`: model `i`, which must be a design model ("DGN writer accepts design models only"); out of range: "model index i out of bounds (n models)". Before, every document with more than one model failed. |
| `dimension_policy` | `Strict` (default): "source and seed model dimensions differ" as before. `Promote2dTo3d`: a 2D model goes into a 3D seed with the Z it has (0 for 2D geometry: no coordinate changes), a 3D model into a 2D seed only when every Z is 0. `ProjectToSeed`: as `Promote2dTo3d`, and 3D geometry is projected onto `z = 0` for a 2D seed (the projection `prepare_document` uses: every Z set to 0 and tilted normals to `+Z`, so a tilted circle, arc or text keeps its size in the XY plane instead of becoming its projection). |
| `estimate_text_length` | center/right text without `dgn.text_length` is written with `characters × character width` (height × width factor), the estimate the reader and left text already use, stored at `0x80`; it reads back with the same anchor. MicroStation's measured lengths are 0.6–0.9 of this estimate ("Mapping decisions"); an application that measures its glyphs places the text from the stored origin `anchor − fx·estimate` and draws its anchor `fx·(estimate − measured)` back along the baseline: 0.05–0.2 of the estimated length for center text, 0.1–0.4 for right text. |

`write_v8_report` / `write_v8_with_seed_report` return the bytes `write_v8` returns
together with the index of the model written and warnings: `dgn.model_not_written` per model
left out (`object` = its index; the message names kind, name and top-level entity count),
`dgn.model_dimension_changed`, and `dgn.text_length_estimated` with the number of texts.
`write_v8` and `write_v8_with_seed` keep their signature and return only the bytes.

`prepare_document` also clears the id of every `Image` (groups included) of a document read
from any format but DGN V8 (DWG, DXF, DGN V7, CityGML, and formats added later): such an id is a handle of the source file, not a seed
element id, and made the writer look for a seed attachment to preserve ("adding/removing
seed rasters requires rewriting their native control graph"); the images become new
attachments. Documents read from DGN V8 and documents of unknown origin keep their ids. The
writer preserves a seed attachment only when the document holds every image of the seed,
each unchanged (same id, kind, symbology and props): a document read from the seed's own
file writes back with its rasters; one read from another DGN V8 file with rasters still
fails with the error above (its ids name elements of that file, which cannot be told apart
from the seed's), and clearing the ids of its images, as for other formats, makes them new
attachments.

Public samples (40 DXF/DWG files of `corpus/public/acadsharp` and `corpus/public/tika`,
read with cadkit and written into `strip_seed_v8(test_dgnv8.dgn)`, a 3D seed in metres):

| Path | Result |
|---|---|
| `write_v8`, defaults (before) | 0 of 40: "DGN writer requires exactly one source model" |
| `write_v8`, defaults | 0 of 40, past the model choice: the 26 2D files (every DWG) fail with "source and seed model dimensions differ", the 14 3D files (every DXF) with "source and seed units differ; convert coordinates explicitly" (12) or "resolve ByBlock color before DGN export" (2 R12 files) |
| + `Promote2dTo3d` (+ `estimate_text_length`: no change) | 0 of 40: "source and seed units differ ..." (37), ByBlock (2), the raster error above (1) |
| + `Document::convert_units` to the seed unit by the caller | 2 of 40 (4 and 0 entities); the others fail on content `write_v8` does not convert: "DGN lineweight needs an explicit entity/layer dgn.weight index; millimeter mapping is display-dependent" (11), "DGN V8 writer: mtext requires explicit conversion" (4), "DGN V8 writer: insert requires explicit conversion" (1), "RGB color is absent from seed palette; provide dgn.color_index explicitly" (1), "resolve ByBlock color before DGN export" (2), and "adding/removing seed rasters requires rewriting their native control graph" (19, every acadsharp sample from R14 on) |
| `prepare_document` then `write_v8_with_seed` (before: 21 of 40, 19 raster errors) | 40 of 40; read back, every file has as many top-level entities as the prepared model (125 for each acadsharp R14+ sample, 2,143 for R12, 0 to 6,995 for tika) |

Private corpus, every file read and written back into itself (`clear_seed_model`): 556 of
567 write (528 when "Level colors" was recorded; the 28 more are the files with one-vertex
shapes, which were `Unknown` before), 11 fail with "nonzero Z cannot be written into a 2D
seed". No private file holds more than one model.

### Structural report

`check_structure(bytes, &ReadOptions) -> Result<StructureReport>` (crate root and
`native::structure`; no `write` feature needed; `cadkit check-dgn` prints it as JSON)
reports the container and pages of any V8 file without changing it. `Err` only when the
file is not a readable DGN V8 compound file or a limit is exceeded. Fields: CFB version;
storages with a non-empty child tree; how many of those are ordered red-black trees and how
many have the top-down shape; unused slots not in the free form; whether the entries use
the writer's slot order; page count and pages with a non-zero page number or population
(information, not a warning); graphic and control records without a positive time; split
dictionary pages. Every directory slot is claimed by the first sibling tree that reaches it,
so the work is proportional to the directory even when many storages point at one subtree
(debug build, `cadkit check-dgn` on synthetic files of m storages sharing one subtree of n
streams, m = n: 500 in 4.3 s before and 0.03 s after, 1,000 in 21.2 s and 0.06 s, 2,000
(521 KB) in 0.11 s after; before, each storage analysed the shared subtree again).
Warnings (stable codes):

| Code | Meaning |
|---|---|
| `dgn.cfb_tree_unordered` | a sibling tree is not ordered by CFB name, reaches a slot twice or points outside the directory: lookups can miss entries |
| `dgn.cfb_tree_unbalanced` | ordered, but the colours break a red-black rule (black tree root, no red parent of a red node, equal black height); MS-CFB permits this |
| `dgn.cfb_tree_shared` | a storage's sibling tree reaches entries that an earlier storage's tree holds (an entry with two parents); the tree is counted as a storage but not analysed |
| `dgn.cfb_free_entry` | an unused slot is not in the MS-CFB 2.6.3 free form |
| `dgn.record_time` | a graphic or control record has no positive finite time at `+0x18` (`object` = first record id) |
| `dgn.dictionary_page_split` | a `Dgn^Nm` page starts with a complex component |
| `dgn.model_page_counter` | the model header prefix's first dword lies outside [pages holding records, pages] of the model's graphic and control pages |

Observed: the public test_dgnv8 (3 slots) and Water (1 slot) report `dgn.cfb_free_entry`,
TreeText reports nothing; the private corpus reports `dgn.cfb_tree_unbalanced` in 9 files
and `dgn.cfb_free_entry` in 5, nothing else. Every 0.5.2 repack and write of the public
samples reported `dgn.cfb_free_entry`, and the writes of TreeText and Water into
themselves also `dgn.model_page_counter`; current repacks and writes of all three, and
writes into 274 private plan seeds, report no warning (`tests/structure.rs`,
`tests/private_dictionary.rs`). A clean report says the container is consistent; it is
not evidence that an external application accepts the file.

### Seed stripping

`strip_seed_v8(seed, &StripSeedOptions, &ReadOptions) -> StrippedSeed` (crate root, facade
`cadkit::strip_dgn_seed`, CLI `cadkit strip-seed`, Python `strip_dgn_seed`, WASM
`stripDgnSeed`, C `cadkit_dgn_strip_seed`) copies a seed without the drawing content of its
model and without the document properties that name people or organisations. It does not
make a seed anonymous: the names in the kept tables remain, and `StripSeedReport` counts
them for the caller to review. Nothing here was opened in another application.

Steps, each through code the writer already uses:

1. `V8Seed::parse`: a seed the writer rejects (several models, an undecodable page, a file
   header that is not 1,576 bytes) is rejected with the same error.
2. With `keep_cell_definitions = false`, every shared cell definition (a type 34 record
   outside a complex element and the components after it, also across a page boundary)
   leaves the dictionary; changed pages are rewritten in the writer's page form ("Dictionary
   pages") and the XAttributes of the removed records go with them.
3. The writer writes an empty document of the seed's units and dimension into the seed
   (`clear_seed_model`, `preserve_seed_controls = keep_controls`). The model keeps one empty
   graphic page `Dgn^G/$1` (header only); extents are 0; the model header prefix's page
   counter is the number of pages holding records; `Dgn~H` `0x128` keeps the seed's id
   high-water mark (at least every kept id). With `keep_raster_template`, the seed's
   raster attachments are passed as unchanged `Image` entities (tags dropped), which the
   writer keeps byte for byte; it keeps a seed's attachments only all together.
4. The dictionary streams (`Dgn^Nm/*`, `Dgn^NmA/*`) are restored from the seed (after step
   2). The writer repairs some tables when it writes into a seed (a second level table,
   repeated level codes, "Second level table"); the empty write changed 320 dictionary pages
   of the private corpus that way. The stripped seed keeps the original pages, and the
   repairs happen when it is used, as for any seed.
5. Times (below), the property sets (below) and, with `model_name`, the model name are
   replaced.
6. The container is built anew from the remaining streams (`writer/container.rs` `build`):
   the same slot order, top-down red-black trees and free slots as the seed-based rewrite
   (a test compares both layouts on the three public V8 samples), CLSIDs null, state bits 0.
   Only the given streams are written, so no byte of a removed stream reaches a free sector
   or a free directory slot (the synthetic tests search the output for marker bytes of the
   removed property sets). Storages left without streams (the emptied `Dgn^C`, `Dgn^GA`,
   `Dgn^CA`) are dropped; test_dgnv8 (ODA) has no `Dgn^C` storage either.

With the default options the result is accepted by `V8Seed::parse` as an empty seed (with
`keep_controls` or `keep_raster_template` it keeps those records, so it is not empty);
with any options it reports no warning in `check_structure`, and stripping it again with
the same options returns the same bytes.

| Part | Default | Option | Evidence / reason |
|---|---|---|---|
| Graphic records and their XAttributes | removed | `keep_raster_template` keeps the raster frames (94) and their XAttribute 170 | drawing content |
| Control records 66 (level 20) | removed | `keep_controls` | 1,594 records in 567 of 567 private files, 1 each in TreeText and Water (all kept by `keep_controls`, see below); not decoded (model settings; the research pass saw coordinate-system-like content), not needed by the writer, may describe a location |
| Control records 100 with 99 components | removed | `keep_controls` | 350 + 2,800 records in 263 private files; not decoded |
| Control records 39 (tag set definitions in the model) | removed | `keep_controls` | 274 private files; the writer resolves tag sets in the dictionary only, and the dictionary's tag sets are kept |
| Raster control groups 90, 91 × 5, 92, 93 × 2 | removed | `keep_raster_template` (not `keep_controls`) | 2,457 groups in 339 private files; they reference the removed frames and hold file names and `file://` paths |
| Shared cell definitions (34) | kept | `keep_cell_definitions = false` removes them | test_dgnv8 holds 1; no private file holds one |
| Level, font, colour, text style and other type 96 tables, tag sets (39), views (97/98/99), settings (9), dictionary type 66 and 107 records | kept byte for byte | — | what a seed is for; 567 of 567 private dictionaries and the 3 public ones compare equal |
| Units, global origin, UOR, 2D/3D flag, every other model header field | kept | — | the model header element is kept except the fields below |
| `Dgn~S`, `Dgn~Mf`, `Oda~SH`, `DgnTk`, other undecoded streams | kept byte for byte | — | not decoded; counted as `unknown_streams` (2 per private file, 3 per public file) |
| Model index entry | kept, renamed with `model_name` | — | the entry's tail after name and description (26–94 bytes in every private file, none in the public ones) is not decoded and kept |

#### Kept control records

`keep_controls` (and the writer's `preserve_seed_controls`, on by default) keeps every
non-raster control record of the model byte for byte except one whose dependency linkage
names a graphic of the seed as a root (`writer/raster.rs` `references_graphics`; a linkage
with an undecoded root type is searched byte window by byte window). Raster control groups
(90–93) follow the raster rules instead. Up to this change every 8-byte window of the record
from `0x18` was compared with the seed's graphic ids, aligned or not, and a match dropped the
record. Measured over the non-raster control records of the model (own structural scan,
counts and in-record offsets only):

| | TreeText, Water | Private corpus (567 files) |
|---|---|---|
| non-raster control records | 1 + 1 (type 66) | 5,018 |
| records with a dependency linkage | 0 | 0 |
| records with a byte window equal to a graphic id | 2 (51 and 19 windows, at every alignment) | 83 (53 × 66, 18 × 99, 12 × 100; 108 windows at 19 positions) |
| what the windows hold | the exponent bytes of a double followed by zero bytes (for example the high bytes `b9 3f` of 0.1 before an all-zero double) | at all 19 positions the matching value also occurs at the same offset in records of the same type and length whose file has no graphic with that id; 11 positions hold one value in every record of that shape |
| kept by `keep_controls`, window rule | 0 | 4,935 |
| kept by `keep_controls`, dependency rule | 2 | 5,018 |

So no observed window match was a reference: graphic ids are small integers, and small
integers and double exponents fill undecoded fields. The window rule also split control
groups: 6 of the type 99 components it dropped belonged to a type 100 group whose header it
kept. A record that refers to a graphic by an undecoded body field would now be kept; none
is known, and the writer does not interpret those records. Writes into a seed whose model has no graphic
record (an emptied template) are unchanged: both rules keep everything there (the
private corpus stripped with `keep_controls` by either rule, written into with a line
before and after the change: 1,134 of 1,134 outputs byte-identical).

#### Times

`timestamp_ms` 0 takes the seed's file header time; a positive value below the newest time
of a kept record is raised to it. The time is written to:

| Field | Evidence |
|---|---|
| `Dgn~H` `f64` at `0x130` (file save time) | a plausible millisecond time in 570 of 570 V8 files, never older than a record of the file (570 of 570) |
| model header `f64` at `0x88` (model time) | a plausible time in 570 of 570 files, equal to `Dgn~H` `0x130` in 529 and not older than a record in 568 |
| model header `f64` at `0x18`, model index entry `+8` | the element time: zero in 567 of 567 private files (left zero), a time in the 3 public files (replaced); the index entry's `+8` equals it in 557 of 570 files and follows it only where it was equal |
| compound file storages (creation and modification FILETIME) and the root entry (modification time only) | [MS-CFB] 2.6.1: the root's creation time MUST be zero and stream entries keep zero times. The `cfb` crate reads a stream entry's times as zero (its strict mode rejects others), so the seed-based rewrite writes them zero as well, although the observed MicroStation files store a time on every entry; the seed-based rewrite keeps the seed's root and storage times (all three public seeds store 1970-01-01 in the root's creation time) |
| `\u{5}SummaryInformation` creation and last-save time | [MS-OLEPS] `PIDSI_CREATE_DTM` / `PIDSI_LASTSAVE_DTM`; omitted when there is no time |

Records kept from the seed (dictionary, kept controls) keep their own times.

#### Property sets

Every observed V8 file has `\u{5}SummaryInformation` and `\u{5}DocumentSummaryInformation`
([MS-OLEPS]): byte order `0xFFFE`, version 0, system identifier `0x00020005`, null CLSID,
code page 1200, the application name as `VT_LPWSTR` (3 public files; [MS-OLEPS] 2.25.1
gives `PIDSI_APPNAME` the type `VT_LPSTR`). The private files
also hold author (`0x04`) and last author (`0x08`) in 567 of 567, a thumbnail (`0x11`) in
449, title, subject, keywords, comments and template (`0x02`–`0x07`) in 274, company
(`0x0F`) in 567 and manager (`0x0E`) in 274; 326 carry a third, user-defined property set
stream. Each public sample names a person or a company and holds a thumbnail.

Stripping removes every property set stream and writes `\u{5}SummaryInformation` with the
code page, the application (`"cadkit"` by default; empty leaves it out) and the two times,
and `\u{5}DocumentSummaryInformation` with the code page only (`writer/oleps.rs`; layout in
its module comment). The application name is written as the specification's `VT_LPSTR`
(a 2.5 `CodePageString`: under code page 1200 NUL-terminated UTF-16LE characters with a size
in bytes), not as the observed `VT_LPWSTR` (a size in characters); whether an application
that reads the observed form also reads this one is untested. The locale property
(`0x80000000`, in every observed file) is not written. The reader reads the code page and
the application back in both forms (`VT_LPSTR` as UTF-16 under code page 1200, as 8-bit
text otherwise).

#### Corpus results

Public (`tests/strip_seed.rs`; sizes in bytes): test_dgnv8 27,648 → 7,680 (58 graphic
records removed; 1 level, 1 font, 1 cell definition kept), TreeText 847,360 → 8,192 (33,335
graphic and 1 control record removed), Water 3,840,000 → 11,264 (66,053 graphic records,
66,053 XAttributes and 1 control record removed). All three: clean `check_structure`, the
dictionary byte for byte, idempotent, and `write_v8` writes into the result without
`clear_seed_model`.

Private (567 V8 files, counts only): 567 strip, none is rejected; 1,339,778 graphic records
(195–13,660 per file), 27,131 control records, 2,031 XAttributes and 1,460 property set
streams removed; 1,336,990 dictionary records with 205,849 named levels (151–1,273 per
file), 6,208 tag sets and 1,701 fonts, 2 undecoded streams per file and 274 model
descriptions remain; 115,195,778 → 41,892,864 bytes (62,464–95,232 per file). Every result has a clean `check_structure` and the seed's
dictionary byte for byte; every 16th is checked for idempotence, a `write_v8` into it and
the `keep_controls` / `keep_raster_template` variants (a run of the CLI over every 8th file
kept 176 attachments).

Open: which removed parts a receiving application needs (the model settings records 66 and
100, the locale property, the dropped empty storages) is unknown until a stripped seed is
opened, filled and saved there (acceptance kit files 02 and 03, [ACCEPTANCE.md](ACCEPTANCE.md)); the contents of `Dgn~S`, `Dgn~Mf`, the model index entry
tail and the undecoded tables are kept unread and may identify the source.

### Built-in blank seed (not built)

A seed shipped with cadkit, used when the caller has none, may hold only fields cadkit decodes
and can set from their documented meaning, and byte blocks shown to be format constants: equal
in the public V8 samples of both writer families, test_dgnv8 (ODA Teigha DGN 4.02, 3D) and the
MicroStation files TreeText (8.11) and Water (8.1), both 2D, with the private corpus as
corroborating counts only. Bytes of third-party default templates (whose licence is undecided)
are not embedded, and undecoded fields that differ between the writers are neither embedded,
zero-filled nor guessed (maintainer decision, 0.5.3).

Under these rules no blank seed can be built today, and cadkit ships none: `Dgn~H` and
`Dgn~Mh`, which every seed needs (`V8Seed::parse` requires both), hold undecoded fields whose
values differ between the public writers. `write_v8` still takes a caller seed;
[`strip_seed_v8`](#seed-stripping) prepares one from the caller's own file. The comparison is
`tests/blank_seed_evidence.rs` (own hex analysis; nothing of the samples is copied into the
sources).

Format constants (equal in the three public samples; private counts over 567 V8 files):

| Stream | Offset, length | Content | Private |
|---|---|---|---|
| `Dgn~H` stream prefix | `0x00`, `0x14` bytes | `01 00 04 00`, then 16 zero bytes | 567 of 567 equal |
| `Dgn~H` inflated (1,576 bytes) | 381 of its 394 dwords: all but the 4 decoded ones (`0x128` id high-water mark, `0x130` save time) and the 9 below | `u32` 8 at `0x114`, `u32` 2 at `0x11C`, the other 379 dwords zero | 567 of 567 equal |
| `Dgn~S` | 208 bytes | every byte `0xFF` | 276 of 567 (the others differ) |
| `Dgn~Mf` inflated | 276 bytes | `10 01`, then 274 zero bytes | 567 of 567 equal |
| `Dgn~Mh` stream prefix | `0x1004` bytes, all dwords but the first three | zero (dword 0 is the page counter, "Model header") | 567 of 567 |
| `Dgn~Mh` element, fixed part | `0x1F0` bytes (attribute offset `0xF8` words in all three) | of its 124 dwords, 24 belong to decoded fields (type word, length, id, element time, sub unit numerator, model time, extents, UOR per master) and 8 to the differing fields below; the other 92 are equal | 91 of the 92 equal in 567 of 567; `u32` `0x38` (0 in the public samples) in 25 |

Undecoded fields that differ between the public writers:

| Stream | Field | test_dgnv8 (ODA) | TreeText (MS 8.11) | Water (MS 8.1) | Private (567) |
|---|---|---|---|---|---|
| `Dgn~H` | `u32 0x110` | 0 | 0 | 1 | as Water in 567 |
| `Dgn~H` | `u32 0x118` | 5 | `0x67` | 5 | one value in 567, none of the public ones; equal to the model header prefix `+8` |
| `Dgn~H` | `u32 0x138` | 0 | 1 | 1 | 72 distinct values; as the MicroStation files in 293 |
| `Dgn~H` | `u32 0x13C` | 3 | 0 | 0 | 2 distinct values, neither public one |
| `Dgn~H` | `u32 0x140` | 0 | 6 | 4 | 52 distinct values |
| `Dgn~H` | `u32 0x308` | 0 | 2 | 3 | as TreeText in 565 |
| `Dgn~H` | `u32 0x310` | 0 | 1 | 1 | as the MicroStation files in 537, as ODA in 30 |
| `Dgn~H` | `u32 0x314` | 2 | 0 | 0 | as the MicroStation files in 567 |
| `Dgn~H` | `u32 0x388` | 0 | 1 | 0 | as TreeText in 567 |
| `Dgn~Mh` prefix | `u32 +4`, `u32 +8` | 0, 0 | 8, `0x67` | 0, 0 | equal to `Dgn~H` `0x114` / `0x118` in 567 |
| `Dgn~Mh` element | `u32 0x20` | 4 | 6 | 4 | as TreeText in 567 |
| `Dgn~Mh` element | `u32 0x44` | 1 | 0 | 1 | as ODA and Water in 567 |
| `Dgn~Mh` element | `f64 0xF8` | 1e7 | 0 | 0 | as the MicroStation files in 567 |
| `Dgn~Mh` element | `f64 0x120`, `0x140`, `0x160` | 0 | 1.0 | 1.0 | as the MicroStation files in 567 |
| `Dgn~Mh` element | `f64 0x1C8` | 0 | 1.0 | 0 | as TreeText in 567 |
| `Dgn~Mh` element | `f64 0x1D8` | 0 | −1000 | 0 | 0 in 556, −1000 in 11 |

The type word of a 3D model is not settled either: bit `0x00400000` (meaning unknown, "Model
header") is clear in the three public samples and set on every private 3D model (556), and no
public MicroStation sample is 3D. The model index entry (`Dgn~Mix`, "Model index") differs at
`+0x00` (`0x00010000` in test_dgnv8 and Water, 0 in TreeText) and `+0x1C` (non-zero in TreeText
only), and private entries carry a 26–94-byte tail the public ones lack. No allowed source
decodes these fields (ezdgn's V8 notes list no `Dgn~H` layout and, for the model header, only
the fields cadkit already reads).

What the remaining parts would take: the compound file (`writer/container.rs` `build`), the
`[MS-OLEPS]` property sets (`writer/oleps.rs`), an empty graphic page, the page counter and a
level table could be produced from decoded meaning; the dictionary records a vendor template
carries are template content and would be absent. Records the public samples carry (sizes in
bytes):

| Record | test_dgnv8 | TreeText | Water |
|---|---|---|---|
| type 9 settings | 10,344 | 10,344 | 10,344 |
| type 97 view group | 216 | 264 | 264 |
| type 98 views (8) | 2,432 | 2,432 | 2,432 |
| type 99 (8) | 480 | 672 | 482 |
| table 4 header and entries | 1,768 | 1,280 + 144 | 1,288 + 72 |
| tables 3, 5, 6, 7, 19 headers and entries | — | 40; 64 + 792; 48 + 408; 40 + 224; 48 + 144 | —; 64 + 792; 48 + 400; 40 + 5,152; — |
| font table (table 2) | 40 + 56 | — | — |
| dictionary type 66 records | — | 1,404 (level 26), 1,120 (level 20) | — |
| model control 66 (level 20) | — | 2,404 | 1,532 |
| type 5 colour table | — | — | — (868 in every private file) |
| `Oda~SH` / `DgnTk` | 16 / — | — / 64 | 16 / — |

Level entries differ in layout between the writers (ODA 136 bytes, the two MicroStation files 128
bytes each), so no level record is a format constant either. Settled by: the meaning of the
fields above (open question 17), or a maintainer decision to embed values from a source whose
licence is settled.

### Evidence and acceptance boundary

Synthetic tests cover 2D/3D models, Unicode and CP1254 tags/text, tag dependencies,
palette mapping, holes, wrapped arc angles, limits and truncated seeds. Public
GDAL `test_dgnv8.dgn` is tested for repack preservation, new geometry, deterministic
output and independent `cfb::CompoundFile::open_strict`. Existing public CSV
reader-oracle tests remain separate. The private corpus tests (`tests/private_*.rs`,
`tests/structure.rs`, `tests/strip_seed.rs`) run when `corpus/private` is present, and one
writer test checks a private seed in memory when `CADKIT_PRIVATE_DGN` names one; neither
copies anything into fixtures or outputs.

These tests establish local container/reader agreement. They do not establish
MicroStation or CityMax acceptance, unknown-field correctness for all V8 files,
or byte-identical regeneration of vendor-written elements.

The acceptance kit (public seed, invented content; [ACCEPTANCE.md](ACCEPTANCE.md) lists its
14 DGN files and the checks for each) is generated and verified with:

```sh
python3 scripts/buildlock.py cargo run -p cadkit --example interchange -- \
  corpus/public/gdal/test_dgnv8.dgn target/interchange-validation
```

Every kit file reads back equal to what was written, reports no `check_structure` warning
and opens with `cfb::CompoundFile::open_strict`; `crates/cadkit/tests/acceptance_kit.rs`
also requires byte-identical regeneration. Before claiming external acceptance, follow
[ACCEPTANCE.md](ACCEPTANCE.md): open every kit file, record the checks, save in the
application and compare the saved copies with cadkit. No external test environment is
currently available. No compatibility certification is claimed. The `dgn_write` fuzz target also exercises reading,
seed-based writing and repacking under reduced resource limits; short
coverage-instrumented AddressSanitizer runs complement the deterministic tests.

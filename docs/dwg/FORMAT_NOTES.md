# DWG format notes (cadkit-dwg)

What `crates/cadkit-dwg` implements, per version, with spec references and the
evidence behind every deviation from the spec.

- Primary reference: ODA *Open Design Specification for .dwg files* v5.4.1
  (`refs/oda-open-design-spec-dwg.pdf`, page numbers below are PDF pages).
- Secondary reference: ACadSharp (MIT, © DomCR), `src/ACadSharp/IO/DwgReader.cs` and
  `IO/DWG/DwgStreamReaders/*` — used to resolve spec ambiguities. Ported logic is
  marked in the code: the R2007 LZ77 decompressor (`container/lz77_r2007.rs`) and the
  R2007 container flow (`container/r2007.rs`).
- Evidence: the ACadSharp samples `corpus/public/acadsharp/sample_AC10xx.dwg` with their
  ASCII DXF exports (same drawing), and 19 Apache Tika test drawings (below). Tests:
  `crates/cadkit-dwg/tests/{corpus,oracle,robustness}.rs`.

## Status

| Version | Container | Header vars | Classes | Objects decoded | Whole-document comparison with the DXF export |
|---|---|---|---|---|---|
| AC1012 (R13) | implemented (same path as R14) | implemented | implemented | implemented | **untested: no permissively licensed public sample found** (see below) |
| AC1014 (R14) | ✓ CRC ok | ✓ 184 vars, CRC ok | ✓ 54 | 1242/1242, 0 errors | no DXF export; checked against the R2000 export (names, counts, layer flags) |
| AC1015 (2000) | ✓ CRC ok | ✓ 218 vars, CRC ok | ✓ 51 | 1047/1047, 0 errors | ✓ 0 mismatches |
| AC1018 (2004) | ✓ CRC32 + page checksums ok | ✓ 229 vars | ✓ 51 | 1014/1014, 0 errors | ✓ 0 mismatches |
| AC1021 (2007) | ✓ (CRC64s not checked) | ✓ 270 vars | ✓ 50 | 925/925, 0 errors | ✓ 0 mismatches |
| AC1024 (2010) | ✓ | ✓ 275 vars | ✓ 50 | 923/923, 0 errors | ✓ 0 mismatches |
| AC1027 (2013) | ✓ | ✓ 277 vars | ✓ 50 | 865/865, 0 errors | ✓ 0 mismatches |
| AC1032 (2018) | ✓ | ✓ 277 vars | ✓ 50 | 842/842, 0 errors | ✓ 0 mismatches |

Decoder completeness is checked independently of any oracle: from R2000 the main-data
size of every object is exact, and every complete decoder consumes it to the bit (0 bits
left; 1 in R2007+ objects without strings = the string-present flag) and its handle
stream up to the byte padding — asserted for every decoded object of every sample in
`complete_decoders_consume_their_streams_exactly` (`DwgObject::data_bits_left`,
`handle_bits_left`). Errata 16–19 and 21 were found this way.

### Whole-document comparison (`tests/oracle.rs`)

Every entity of the DXF reader's document (`cadkit_dxf::read` on `sample_<ver>_ascii.dxf`)
is matched by handle with the DWG reader's document and compared in serialized form
(kind with all geometry, layer, color, linetype, lineweight, visibility, attributes;
floats within 1e-9 relative). The counts are the same for all six versions:

| Area | Entities | Exact | Float noise only | Documented difference | Mismatch / missing |
|---|---|---|---|---|---|
| model space | 163 | 39 | 100–101 | 23–24 | 0 |
| paper space (3 layouts) | 6 | 0 | 6 | 0 | 0 |
| block definitions | 172 | 30 | 109 | 33 | 0 |

"Float noise" is the 16-digit decimal round trip of the DXF text. Each documented
difference is a narrow rule in the test that checks its own evidence before it
reconciles the two sides (the trees are compared again afterwards):

| Entities per version | Difference | Who is right |
|---|---|---|
| 52 | TEXT/MTEXT style: the DXF export omits group 7 for the default style `Standard`; the DWG stores the STYLE handle | both (the DWG reader names it) |
| 3 | block `*U25` (ACAD_TABLE contents): the DXF export regenerated the anonymous block with new handles; the DWG block holds the same entity kinds | both |
| 1 | LEADER: the DXF export adds the hook-line vertex (arrow size before the last point) that the DWG does not store | both |
| 1 (AC1027, AC1032) | SPLINE 434: the R2013+ DWG stores a fit-point definition (fit points + tangents), the DXF export only the computed control polygon | both |

The 3 DWG `*U25` entities have no DXF counterpart by handle and are listed separately.
Two earlier AC1032 differences (MTEXT direction read from the R2018 embedded object,
and the multi-line ATTRIB value) were DXF reader issues, since fixed there; their
rules were removed, and AC1032 now compares exactly like AC1027.

### Additional public samples (Apache Tika, Apache-2.0)

19 DWG test documents of Apache Tika
(`https://github.com/apache/tika/tree/main/tika-parsers/tika-parsers-standard/tika-parsers-standard-modules/tika-parser-cad-module/src/test/resources/test-documents/`:
`testDWG2000.dwg`, `testDWG2004.dwg`, `testDWG2004_no_header.dwg`, `testDWG2007.dwg`,
`testDWG2010.dwg`, `testDWG2010_custom_props.dwg`, `testDWG-AC1027.dwg`,
`testDWG-AC1032.dwg`, `testDWGmech6.dwg`, `testDWGmech2004.dwg` … `testDWGmech2011.dwg`,
`architectural_-_annotation_scaling_and_multileaders.dwg`; 2 × AC1015, 6 × AC1018,
5 × AC1021, 4 × AC1024, 1 × AC1027, 1 × AC1032; 91 924 objects)
were used as unseen-file validation: all decode with complete headers (CRC ok), every
object parsed, no warnings, every complete decoder consuming both streams exactly
(`extra_public_samples_decode_cleanly`; reads `corpus/public/tika/` when the fetch script
provides it, or `CADKIT_DWG_EXTRA_SAMPLES=<dir>`). Two of them exposed erratum 23. They
have no DXF export, so only structure is checked.

### R13 (AC1012)

No permissively licensed R13 sample was found: ACadSharp ships AC1014+, Apache Tika
AC1015+, and the R13 files circulating in other projects originate from LibreDWG's test
data (GPL, off limits). The R13 path differs from R14 only in the version gates the
spec lists (DICTIONARY without the R14-only `RC`, LEADER without the annotation
offset); it is untested.

## Bit codes (spec chapter 2, p. 6–18)

`bits.rs`. Bit 7 of the first byte is read first; raw multi-byte values embedded in the
bit stream are little-endian byte sequences that may straddle bytes.

| Code | Notes |
|---|---|
| BS/BL/BD | 2-bit prefix; BL/BD code `11` is unused and rejected |
| BLL | **3 fixed bits** of byte count (spec §2.4 says a `3B` code — erratum 4) |
| DD | `01` patches bytes 0..4, `10` patches bytes 4,5 then 0..4, `11` full RD |
| BT/BE | R2000+: one flag bit for 0.0 / (0,0,1); R13–R14 plain BD / 3BD |
| MC | groups of 7 bits LSB first; final byte bit 0x40 = negative (6 value bits) |
| UMC | object map handle deltas and R2010+ handle-stream size: no sign bit |
| MS | 16-bit LE modules, bit 15 = continue |
| H | `code:4 counter:4` + counter bytes big-endian; codes 6/8/A/C relative to the object's own handle |
| OT | R2010+: `BB` 0 → byte, 1 → byte+0x1F0, 2/3 → raw short |
| T / TU | T: BS byte length + 8-bit chars (code page); TU: BS char count + UTF-16LE |
| CMC | ≤R2000 BS; R2004+ BS index, BL value (high byte method: C0 bylayer, C1 byblock, C2 RGB, C3 ACI, C8 none), RC flags, [TV name], [TV book] |
| ENC | R2004+ BS with flags in the high byte: 0x8000 → BL RGB follows; **0x4000 → DBCOLOR handle in the handle stream and no RGB value** (erratum 14); 0x2000 → BL transparency |

CRCs (`crc.rs`): the "8-bit CRC" is a CRC-16/ARC table CRC (objects/header/classes
seed 0xC0C1, R13 file header seed 0 XOR a count-dependent magic); CRC-32 is the
standard reflected one; the R2004 page checksum is an Adler-like sum with 0x15B0 chunks.
All of them validate on every sample (asserted in `every_sample_decodes_cleanly`).

## R13–R15 container (chapter 3, p. 19–21)

| Offset | Size | Field |
|---|---|---|
| 0x00 | 6 | magic |
| 0x0B | 1 | maintenance release (R14 sample: 10, R2000 sample: 15) |
| 0x0D | 4 | image seeker |
| 0x13 | 2 | DWGCODEPAGE index (30 = ANSI_1252) |
| 0x15 | 4 | locator record count (R14 sample 5, R2000 sample 6) |
| 0x19 | 9·n | RC number, RL seeker, RL size |
| … | 2 | CRC-8 seed 0, XOR 0xA598/0x8101/0x3CC4/0x8461 for 3/4/5/6 records |
| … | 16 | sentinel `95 A0 4E 28 …` |

Records: 0 header vars, 1 classes, 2 object map, 3 unknown (size 0 in samples),
4 template (`00 00 00 00`: description length 0, MEASUREMENT 0), 5 (R2000 sample only)
AuxHeader (`FF 77 01 …`). Object map offsets are absolute file offsets.

## R2004 / R2010 / R2013 / R2018 container (chapters 4, 6–8, p. 22–34, 67–69)

`container/r2004.rs`.

- 0x80 bytes plain header: maintenance at 0x0B (R2010 sample 226, R2013 125, R2018 **0**
  — the R2018 maintenance value lives elsewhere), codepage at 0x13, security flags at
  0x18 (bit 0 = encrypted data → `Error::Unsupported`).
- 0x6C bytes at 0x80 XOR the LCG sequence `seed = seed*0x343FD + 0x269EC3; byte = seed>>16`
  starting from seed 1. Decrypted: `"AcFssFcAJMB\0"`, page map id @0x50, page map
  address @0x54 (+0x100), section map id @0x5C, CRC-32 @0x68 computed with the field
  zeroed — matches on all samples.
- System pages: 0x14-byte header (type 0x41630E3B page map / 0x4163003B section map,
  decompressed size, compressed size, compression 2, checksum). Checksum = page
  checksum of the compressed data seeded with the checksum of the header with its
  checksum field zeroed — matches.
- Page map: (i32 number, u32 size) pairs; negative numbers are gaps followed by 16
  bytes; addresses accumulate from 0x100.
- Section map: count + 16 bytes, then per section: u64 size, u32 page count, u32 max
  page size (0x7400), u32 unknown, u32 compressed (2), u32 id, u32 encrypted, 64-byte
  name, then (i32 page number, u32 data size, u64 start offset) per page. Pages are
  placed at their start offsets; omitted all-zero pages read as zeros (see section
  assembly below).
- Data pages: 32-byte header XOR `0x4164536B ^ page address`, type 0x4163043B; data
  checksum (0x1C, seed 0 over the payload) and header checksum (0x18, header with that
  field zeroed, seeded with the data checksum) match the spec on all samples.
- Section assembly (shared with R2007, `container::SectionBuffer`): each page number,
  file page and start offset is used once (repeats get `dwg.duplicate_page`), pages are
  placed in start-offset order, and the buffer grows as pages arrive. The declared
  size is clamped to a plausibility bound (listed pages × min(page size, 256 ×
  compressed size + 1 KiB) + 64 zero pages); bytes no page wrote (gaps, omitted zero
  pages, the tail) are limited to 64 pages and 16 MiB. Page sizes above 1 MiB are
  rejected. Every page decompression is charged to `Limits::max_decompressed_bytes`
  by its actual output, as is the zero fill.
- System pages above 8 MiB (`MAX_SYSTEM_PAGE`; real ones need 8–24 bytes per data
  page) are rejected; the output grows with the data and only produced bytes are
  charged.
- The AcDb:AcDbObjects section starts with RL 0x0DCA; object map offsets are relative to
  the section start.

R2004 LZ77 (`container/lz77_r2004.rs`, §4.7 p. 32–34): see errata 1–2, which
ACadSharp's `DwgLZ77AC18Decompressor` reads the same way. A first byte above 0x11
starts with a literal run of `byte - 17` bytes: not in the spec, taken from ACadSharp's
`DecompressToDest`; none of the 509 compressed pages of the ACadSharp and Tika R2004+
samples starts that way. Every copy is bounds-checked and the output grows with the
data, never past the page size.

## R2007 container (chapter 5, p. 35–66)

`container/r2007.rs`, `container/lz77_r2007.rs`, `container/reed_solomon.rs`.

- 0x400 bytes at 0x80: RS(255,239) with 3 interleaved blocks; byte `j` of block `i` is
  at `i + 3j`. Only de-interleaving is done (no error correction, as ACadSharp).
  Decoded: CRC, key, compressed CRC, i32 compressed length @0x18 (negative = stored),
  then data decompressing to the 0x110-byte record (fields of §5.2 p. 37–39).
- System pages (page map, section map) at `0x480 + offset`: data length aligned to 8,
  repeated `correction factor` times, RS(255,239) interleaved over
  `ceil(total/239)` blocks; first copy decompressed.
- Page map: (i64 size, i64 id) pairs, offsets accumulate from 0; ids may be negative.
- Section map: 8 u64 fields, then the name (**the name length is a byte count of
  UTF-16 incl. terminator**, not characters as §5.2 says), then 7 u64 per page
  (offset, size, id, uncompressed, compressed, checksum, CRC).
- Data pages (encoding 4): RS(255,251) interleaved over `ceil(align8(compressed)/251)`
  blocks; decompressed when compressed < uncompressed.
- The R2007 LZ77 literal copy shuffles bytes in 32/16/8/4-byte groups (§5.10.1 table);
  cadkit precomputes the permutation per length at compile time.
- The file header record is not CRC-checked, so its system page sizes are capped at
  8 MiB; the LZ77 output grows with the data (a declared size never allocates up
  front). Data sections are assembled as for R2004 (unique pages, lazy growth, charged
  decompression, limited zero fill).
- Not done: CRC64 / page CRC verification, encrypted sections. (All R2004-family
  checksums — file header CRC-32, system page and data page checksums — are verified.)

## Header variables (chapter 9, p. 70–83)

`header.rs`. Sentinel, RL size, `[RL]` (R2010/R2013 with maintenance > 3, and R2018),
data, RS CRC (seed 0xC0C1 over everything after the start sentinel up to the data end),
end sentinel — verified on all samples. R2007+: an RL bit size at the start of the data;
the string stream ends at bit `start + bitsize - 1` (flag) and the handle stream starts
right after it. HANDSEED is in the main stream. All variables listed in §9 are decoded
by name into `HeaderVars` and appear as `dwg.header.<NAME>` document props.

Cross-check with the DXF exports: EXTMIN/EXTMAX, INSUNITS (1), LUNITS (2), MEASUREMENT
(template section, 0), FINGERPRINTGUID equal for all versions. HANDSEED differs from the
DXF `$HANDSEED` in every version (DXF export allocates handles) — not a decode error.

## Classes (chapter 10, §5.8, p. 52–53, 84–85)

`classes.rs`. R2004+ records add BL count, BL DWG version, BL maintenance, 2 BL unknown
(the spec's "BS" for the versions reads identically for values < 65536). R2007+: RL bit
size + string stream like the header. Trailer CRC and sentinel verified.

## Object map (chapter 23, p. 243)

`handles.rs`. Chunks of big-endian RS size (size field + pairs; may exceed the spec's
2032-byte cap — erratum 23), UMC handle delta, MC offset delta; **the running handle and
offset restart at 0 in every chunk**. Chunk CRCs (seed 0xC0C1 over size + data, stored
big-endian) match. A map that ends early keeps its entries with a
`dwg.object_map_truncated` warning. Hardening: an entry whose handle or offset is
already mapped is dropped (first wins; one offset holds one object), and entries beyond
`object data size / 4 + 1` (an object takes at least 4 bytes) are ignored, both with a
`dwg.object_map` warning; more than `max_objects` entries is a limit error.

## Objects (chapter 20, p. 100–108)

`object/mod.rs`.

| Stream | R13–R14 | R2000–R2007 | R2010+ |
|---|---|---|---|
| prefix | MS | MS | MS, UMC handle-stream bits |
| type | BS | BS | OT |
| handle stream start | RL after EED (objects) / after graphics (entities) | RL after the type | object end − UMC |
| strings | inline | inline; R2007: string stream | string stream |

The R2007+ string stream is located backwards from the bit just before the handle
stream: if that bit is set, the 16-bit size (in **bits**; §20.1 says "bytes") precedes
it, extended by a second word when bit 15 is set, and the string data precedes that.

Common entity data order as in §20.4.1; the ENC color, then R2000+ linetype flags,
plot style flags, R2007+ material flags and shadow byte, R2010+ three visual-style bits,
invisibility, lineweight. Handle order: owner (entity mode 0), reactors, xdictionary,
[R13–R14 layer, linetype], [≤R2000 prev/next unless no-links], [R2004+ DBCOLOR],
[R2000+ layer, linetype, material, plot style, visual styles]. The R2018 multi-line
ATTRIB/ATTDEF repeats exactly this part (from the entity mode) for its embedded MTEXT.

Registry (`object/registry.rs`): fixed codes per §20.3, class numbers by DXF name,
looked up through a number → class index (`ClassTable`), not a scan per object.

Decoder hardening: counts are checked against `max_vertices` and the bits left
(`checked_count`, with the true minimum bits per item, e.g. 6 per HATCH path and 16 per
HATCH edge); all paths, edges, vertices and spline points of one HATCH count together
against `max_vertices`; SPLINE and HATCH spline-edge degrees outside 1..=25 are
clamped with a `dwg.spline_degree_clamped` warning. Decoder warnings are kept in
`DwgObject::warnings` and reported by the reader.

Warning offsets: `Warning::offset` holds file offsets only. Positions inside a
decompressed section (R2004+ objects, header, classes, object map) are relative to that
section, so the message names it (`AcDb:AcDbObjects+0x1a2b`) and the offset is left
empty; errors from such sections say "offset relative to the section".
**Extension point for new types**: write `fn(&mut Streams) -> Result<ObjectData>`
reading the type-specific fields (`s.main` data, `s.tv()` strings, `s.h()`/`s.h_opt()`
handles, `checked_count` / `read_handles` for file-provided counts — common data is
already consumed), add an `ObjectData` variant (`native/entities.rs`), register it in
`fixed_decoder` / `class_decoder`, map it in `mapping/entity.rs`, and add the type to
`COMPLETE_DECODERS` in `tests/corpus.rs` once it consumes both streams exactly.

Complete decoders (both streams consumed exactly on every sample): TEXT, ATTRIB, ATTDEF
(incl. R2018 multi-line with embedded MTEXT), BLOCK, ENDBLK, SEQEND, INSERT, MINSERT,
LINE, POINT, CIRCLE, ARC, ELLIPSE, SPLINE, LWPOLYLINE, POLYLINE_2D/3D/PFACE/MESH,
VERTEX_2D/3D/MESH/PFACE/PFACE_FACE, SOLID, TRACE, 3DFACE, MTEXT, DIMENSION_ORDINATE/
LINEAR/ALIGNED/ANG_3PT/ANG_2LN/RADIUS/DIAMETER, LEADER, TOLERANCE, HATCH, IMAGE,
WIPEOUT, IMAGEDEF, VIEWPORT, RAY, XLINE, SHAPE, MLINE, OLE2FRAME (not in the samples),
all control objects, BLOCK_HEADER, LAYER, STYLE, LTYPE, APPID, DICTIONARY,
DICTIONARYWDFLT, LAYOUT, DBCOLOR. Partial: VIEW/UCS/VPORT/DIMSTYLE/VP_ENT_HDR (name
only), REGION/3DSOLID/BODY (modeler data header; SAT block sizes for version 1). Every
other type keeps its common data and raw bytes (`ObjectData::Unparsed`): MULTILEADER,
ACAD_TABLE, MESH (subdivision), PDFUNDERLAY, ARC_DIMENSION, LARGE_RADIAL_DIMENSION,
and the non-graphical objects (XRECORD, GROUP, MLINESTYLE, …).

## Text (`codepage.rs`)

- R13–R2004 strings (`T`, EED 1000) are 8-bit in the drawing code page: the index at
  offset 0x13 (DXF names `ANSI_1252` …, table in `codepage.rs`) maps to a WHATWG label
  for `encoding_rs`. DOS code pages other than 866, and Johab, have no WHATWG encoding:
  `ReadOptions::fallback_codepage` is used, else windows-1252, with a
  `dwg.codepage_fallback` warning. Trailing NULs are dropped.
- R2007+ strings are UTF-16LE (`TU`), in the string stream of the object / header /
  classes; EED strings are inline (RS length + UTF-16). Lengths are checked against
  `Limits::max_string_bytes`.
- `\U+XXXX` (UTF-16 unit, surrogate pairs joined) and `\M+nXXXX` (double-byte
  character) escapes are decoded in every string of every version — dimension text
  stores `°` as `\U+00B0` even in UTF-16 files — with `cadkit_core::text::decode_escapes`,
  shared with the DXF reader.
- `SourceInfo::codepage` reports the WHATWG encoding used for 8-bit text.

## Mapping conventions (`mapping/`)

Shared with the DXF reader so both produce the same model (verified by `tests/oracle.rs`):

- OCS → WCS with `cadkit_core::geom_ops::arbitrary_axes` (CIRCLE/ARC centers, TEXT and
  ATTRIB points with elevation, INSERT positions, LWPOLYLINE and 2D POLYLINE vertices,
  SOLID/TRACE corners, HATCH boundaries, DIMENSION points 11/12/16 with the dimension
  elevation, SHAPE); normals are normalized. ELLIPSE, SPLINE, 3D POLYLINE, 3DFACE,
  MTEXT, LEADER, IMAGE, VIEWPORT, LINE, POINT and DIMENSION points 10/13–15 are stored
  in WCS.
- TEXT `position`: the baseline start for left/baseline text and for aligned/fit text
  (whose baseline end is `end_point`), the justification point (11) otherwise; both
  points are also in props (`dwg.insertion_point`, `dwg.alignment_point`).
- MTEXT: `rotation` is the stored X axis measured in the OCS of the normal; `width` the
  reference rectangle width; `line_spacing` 1.0 before R2000; `plain` from
  `cadkit_core::text::plain_text`, shared with the DXF reader.
- POLYLINE_2D/3D → `Polyline` from the VERTEX records (spline-frame control points,
  vertex flag 16, skipped; 2D Z = polyline elevation when non-zero); POLYLINE_PFACE →
  `Mesh` (VERTEX_PFACE positions, VERTEX_PFACE_FACE indices made 0-based, faces with
  at least 3 valid indices); POLYLINE_MESH → `Mesh` with an M×N grid of quads (closed in
  M/N by flags 1/32).
- SOLID/TRACE → `Face { filled }` in outline order 1, 2, 4, 3; 3DFACE → `Face` in stored
  order; a fourth corner equal to the third gives a triangle.
- DIMENSION points, in this order and only when present: 10, 13, 14, 15, 16, then 12 when
  it is not the origin; their roles are in `dwg.dim_roles`. `text_position` = 11,
  `measurement` = actual measurement (R2000+), `text` = user text when not empty,
  `block` / `style` by name.
- HATCH loops: polyline paths → one `HatchEdge::Polyline`; edge paths → Line/Arc/
  Ellipse/Spline edges (angles in radians, ellipse axis converted as a vector);
  `external` = path flag 1 or 16; solid fills have scale 1 and angle 0; pattern lines in
  `dwg.hatch_pattern_lines`.
- IMAGE: `u_vector`/`v_vector` = per-pixel vectors × pixel size; `path` from IMAGEDEF.
- VIEWPORT: center/width/height, view center (DCS) and height; frozen layers by name in
  `dwg.frozen_layers`.
- `Unknown { "dwg.<NAME>" }` with props for ATTDEF (tag, default, prompt, flags,
  position, height — as the DXF reader), RAY/XLINE (point, direction), SHAPE,
  TOLERANCE (text), MLINE (vertices, scale, style), WIPEOUT (frame), OLE2FRAME (data
  size), REGION/3DSOLID/BODY (ACIS version, SAT bytes), and everything else (type code).
  One `dwg.unsupported_entity` warning per type.
- INSERT attributes from the ATTRIB children (owned list R2004+, first..last chain
  before); R2018 multi-line attributes take the embedded MTEXT text.
- Blocks: names from the BLOCK entity; anonymous records store `*U`/`*D`/`*T` and the
  number is the record's index in the BLOCK_CONTROL entry list (null slots count).
  Pre-R2010 files save dynamic blocks as anonymous records whose BLOCK entity keeps the
  dynamic name; the DXF export names them `*U<index>` (sample: `*U17`), and so does
  cadkit (stored name in `dwg.block_entity_name`).
- *Model_Space → `Model` "Model"; every *Paper_Space* block → `Layout` named after its
  LAYOUT object, ordered by tab order, holding its paper-space entities. Space block
  names are in `dwg.block`.
- Membership: R2004+ block headers list their entities; R13–R2000 use owner handle /
  entity mode, ordered by the first..last chain when it covers exactly those entities.
  Each entity is placed at most once, and only in the block that owns it (owner handle,
  or entity mode 1/2 for the paper/model space): repeated references and entities
  owned elsewhere are ignored with `dwg.duplicate_entity` / `dwg.owner_mismatch`
  warnings, so cheap repeated handles cannot multiply the output. Document-wide, more
  than `max_objects` mapped entities or `max_vertices` vertex-like items (vertices,
  points, knots, weights, face indices, hatch edges, attributes) is a limit error.
- Linetype flags 1/2 → "ByBlock"/"Continuous"; lineweight bytes index
  `0,5,9,13,15,18,20,25,30,35,40,50,53,60,70,80,90,100,106,120,140,158,200,211` /100 mm,
  29 BYLAYER, 30 BYBLOCK, 31 DEFAULT. Color-book colors resolve through DBCOLOR.
- Layer description: second string of the layer's `AcAecLayerStandard` EED.
- EED → `dwg.eed.<APPID>` lists.

## Spec errata found (evidence: the samples)

| # | Spec | Files / fix | Evidence |
|---|---|---|---|
| 1 | §4.7: compressed offsets as printed | every back-reference distance is the encoded offset **+1** (also for opcode 0x10–0x1F the +0x3FFF is +0x4000) | R2004+ sections decompress and all object CRCs match |
| 2 | §4.7: opcodes 0x12–0x1F length `(op & 0x0F) + 2` | length is `(op & 7) + 2`; bit 3 is bit 14 of the distance (ACadSharp reads it the same way) | same |
| 3 | §4.1: 0x6C-byte XOR table | the printed table has only 0x5C bytes; use the generator | decrypted header id and CRC-32 |
| 4 | §2.4: BLL length is a `3B` code | 3 fixed bits | R2013+ REQUIREDVERSIONS and R2010+ proxy-graphics sizes (MESH, ACAD_TABLE, MULTILEADER, PDFUNDERLAY) only decode this way |
| 5 | §9: MENUNAME "R13–R18" only | present in R2007+ too (string stream) | without it DIMPOST = "." and FINGERPRINTGUID is shifted; with it all GUIDs equal the DXF |
| 6 | §20.1: string-stream size "decremented by 16 bytes" | 16 **bits** | all R2007+ strings |
| 7 | §5.2: section name length in characters | byte count (incl. terminator) | R2007 section names |
| 8 | §23: one running handle/offset | reset at every chunk (as ACadSharp) | all objects found at their mapped offsets with matching handles and CRCs |
| 9 | §20.4.54: R13/R14 layer "On: 1 if on" | bit set = **off** | R14 layer flags equal the R2000 ones (`r14_layer_flags_match_r2000`) |
| 10 | §20.4.54ff: table entries have `B 64-flag, BS xrefindex+1, B xdep` in all versions | R2007+: only `BS`, xdep = bit 0x100 | R2007+ LAYER/LTYPE/STYLE/BLOCK_HEADER consume both streams exactly and layer flags equal the DXF |
| 11 | §20.4.56 STYLE: vertical then shape-file bit | shape-file first (DXF 70 bit 1), then vertical | `text_styles_match_dxf`: the `.shx` shape entries (DXF 70 = 1) decode as shape files, not vertical |
| 12 | §20.4.9 INSERT: R2004+ owned-object count always present | only when "has ATTRIBs" | every INSERT consumes its main stream exactly (an extra BL would over-read when there are no attributes) |
| 13 | §20.4.52 BLOCK_HEADER: R2004+ owned count always present | absent for xrefs (as ACadSharp) | not exercised by the samples (no xrefs) |
| 14 | §2.11 ENC 0x4000 "0x8000 is also set" | no RGB BL follows; the color is in the DBCOLOR object | CIRCLE 99F (color book) geometry matches the DXF; its DBCOLOR gives DXF 420 |
| 15 | §20.4.1 R2007+ order "plotstyle flags, material flags, shadow" | kept as the spec (ACadSharp reads material, shadow, plotstyle); both read the same bit count; only matters when exactly one is 3 | open (no sample) |
| 16 | §20.4.54 LAYER: a trailing "unknown, always NULL" handle listed as Common | present only from R2013 | handle stream fully consumed only with this rule (R2000–R2010 would over-read, R2013+ under-read) |
| 17 | §20.4.67 DIMSTYLE_CONTROL: entries only | R2000+: an extra `RC` count after the entries and that many extra handles (in the samples a subset of the DIMSTYLE entries; meaning unknown) | both streams consumed exactly; ACadSharp ignores the byte |
| 18 | §20.4.66 APPID | `RC` (undocumented DXF 71) after the xref fields, then the xref block handle | both streams consumed exactly |
| 19 | §20.4.47 LEADER: text box height/width "Common" | present up to R2007 only (as ACadSharp) | LEADER consumes both streams exactly in all versions |
| 20 | §20.4.46 MTEXT: background scale factor `BL` | `BD` (default 1.5, as ACadSharp) | not exercised by the samples (no background fill) |
| 21 | §20.4.40 SPLINE R2013+: "scenario becomes 1 if the knot parameter is custom or has no fit data" | scenario 1 when the knot parameter is 15 or flags1 bit 8 ("use knot parameter") is clear, else 2 (as ACadSharp) | every SPLINE consumes exactly; the R2013+ samples store spline 434 as fit data |
| 22 | §20.4.75 HATCH spline edge R2010+: tangents always follow the fit points | tangents only when there are fit points (as ACadSharp) | not exercised by the samples (no spline edges) |
| 23 | §23: object-map chunks "cut off at a maximum length of 2032" | writers exceed it (a 2033-byte chunk body in Tika `testDWGmech2010.dwg` and `architectural_-_annotation_scaling_and_multileaders.dwg`); read the declared size (ACadSharp caps at 2032 and loses the rest of the map) | both files decode completely only without the cap |
| 24 | §20.4.22 DIMENSION "Flags 1" bit 0 = opposite of DXF 70 bit 128 | as the spec; kept in `dwg.dim_user_text_position` | not compared (the DXF reader keeps the raw flags) |

## Not implemented / open

- R13 (AC1012): same code path as R14 but no permissively licensed sample to validate.
- MULTILEADER, ACAD_TABLE, MESH (subdivision), PDFUNDERLAY, ARC_DIMENSION and
  LARGE_RADIAL_DIMENSION stay `Unknown` without type-specific props (the last two are
  not in any sample). ACIS geometry (REGION/3DSOLID/BODY) is not decoded beyond the
  header; proxy-entity graphics are skipped (size in `EntityCommon::graphics_size`).
- R2007 CRC64 checks, R2004/R2007 encrypted (password) files, AcDb:Security,
  SummaryInfo, Preview, AuxHeader, AcDsPrototype_1b are not read.
- Rules followed from ACadSharp without sample evidence: errata 13, 15, 20, 22 and
  the R2004 LZ77 initial literal run.

## Attributions

- R2007 LZ77 decompressor: ported from ACadSharp `DwgLZ77AC21Decompressor.cs` (MIT,
  © DomCR); the literal shuffle table is restated as data (`COPY_PLANS`).
- R2007 container flow (header RS de-interleave, system page sizing, page/section map
  walk): following ACadSharp `DwgReader.readFileHeaderAC21` / `getSectionBuffer21`.
- R2004 LZ77 (`container/lz77_r2004.rs`): the initial literal run for a first byte
  above 0x11 is taken from ACadSharp `DwgLZ77AC18Decompressor.DecompressToDest`; the
  +1 distances and the 0x12–0x1F length/distance split agree with it (errata 1–2).
- Spec ambiguities resolved with ACadSharp's readers (MIT, `DwgObjectReader`): ENC
  0x4000 handling, R2007+ table-entry xref bits, INSERT owned-object count, BLOCK_HEADER
  owned count for xrefs, STYLE flag order, classes BL/BS equivalence, MENUNAME in R2007+.
  In `object/{curves,annotation,hatch,misc}.rs` exactly four rules come from ACadSharp:
  the R2013+ SPLINE scenario from flags1 bit 8 and the knot parameter (`readSpline`,
  erratum 21), LEADER box height/width only up to R2007 (`readLeader`, erratum 19),
  the MTEXT background scale as BD (`readMText`, erratum 20), and HATCH spline-edge
  tangents only when there are fit points (`readHatch`, erratum 22). Every other field
  of those decoders follows the spec text (§20.4.x as cited in each file), including
  the R2018 multi-line ATTRIB layout and the MTEXT R2018 background/annotative fields.
  Each rule was then confirmed (or contradicted) on the samples as listed in the
  errata table.
- MTEXT plain text and `\U+`/`\M+` escapes: `cadkit_core::text`.
- No GPL/LGPL code or ODA SDK material was consulted.

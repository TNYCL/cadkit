# cadkit-dxf notes

Format knowledge, mapping decisions and verification evidence for `crates/cadkit-dxf`.

## Sources

| Source | Use |
|---|---|
| Autodesk public *DXF Reference* (group codes per entity / table / object) | Meaning of group codes, defaults (applied from knowledge of the public reference; pages were not re-fetched, every mapping is checked against the corpus and ezdxf instead) |
| ACadSharp (MIT) `samples/` | Corpus: `sample_AC1009 … AC1032` as ASCII and binary DXF (`corpus/public/acadsharp`) |
| ezdxf (MIT, Python, installed in `.venv`) | (1) Cross-check of the minimal structure a strict reader expects when *writing* — `ezdxf.new("R2000"/"R2013")` output was inspected for the table set, `LAYOUT` / dictionary objects and `CLASSES`; (2) independent validator (`doc.audit()`) and reader oracle in the dev-time scripts. No ezdxf source was ported. |
| Own hex analysis of the samples | Binary DXF layout, caret notation, handle numbering differences between the two saves |

Forbidden sources (libdxfrw, LibreCAD, QCAD, GPL code) were not consulted.

## Layout of the crate

| Module | Role |
|---|---|
| `native` (public) | `Tokenizer` / `Pair` / `Value` / `code_type`: ASCII + binary group-code stream, text decoding (`encoding_for_codepage`, `decode_escapes`, `decode_carets`). Usable as a scanner/oracle. |
| `reader` | records → header / tables / blocks / entities / objects → `Document`. |
| `ocs` | arbitrary-axis OCS (delegates to `cadkit_core::geom_ops::arbitrary_axes`). |
| `cadkit_core::text` | MTEXT inline-format stripping and the backslash-U-plus / backslash-M-plus escape decoding (shared with the DWG reader; the M-plus code page table is passed in by the caller). |
| `writer` | ASCII writer for R12 … R2018; `write_with_report` also returns the skipped-item warnings. |

## Tokenizer

* Binary files start with `AutoCAD Binary DXF\r\n\x1a\0` (22 bytes). R13+ files use 2-byte group codes
  (first pair is `00 00 "SECTION"`), R12 files 1-byte codes with `0xFF` + 2 bytes for codes ≥ 255
  (first pair is `00 "SECTION"`). Evidence: `sample_AC1009_binary.dxf` vs `sample_AC1015_binary.dxf`.
* Value type by code range (identical for ASCII parsing and binary decoding):

  | Codes | Type |
  |---|---|
  | 0-9, 100-102, 105, 300-309, 320-369, 390-399, 410-419, 430-439, 470-481, 999, 1000-1003, 1005 | string |
  | 10-59, 110-149, 210-239, 460-469, 1010-1059 | double |
  | 60-79, 170-179, 270-289, 370-389, 400-409, 1060-1070 | int16 |
  | 90-99, 420-429, 440-459, 1071 | int32 |
  | 160-169 | int64 |
  | 290-299 | bool (1 byte) |
  | 310-319, 1004 | binary chunk (1-byte length; hex text in ASCII) |

* ASCII: CRLF or LF, leading whitespace in codes, UTF-8 BOM, missing final newline, blank lines between pairs.
  Non-finite numbers become `0.0`. A numeric value that does not parse is kept as `Value::Str`
  (accessors parse leniently).
* **Caret notation** (ASCII only): `^J` is a line feed, `^I` a tab, `^ ` a literal caret. Binary files hold
  the raw character, so decoding it is required for ASCII/binary equality (evidence: MTEXT in
  `sample_AC1018_*`).
* `\U+XXXX` (UTF-16 code unit, surrogate pairs combined) and `\M+nXXXX` are decoded. `\M+` maps `n`
  1/2/3/4 to Shift_JIS / Big5 / EUC-KR / GBK on a best-effort basis (the mapping of `n` is not
  documented in public sources I could use); undecodable escapes stay verbatim.

## Encoding

AC1021+ → UTF-8 (lossy). Older → `$DWGCODEPAGE` (`ANSI_125x`, `ANSI_932/936/949/950`, `DOS866`, `ISO8859-x`,
`UTF-8`, ...) → `ReadOptions::fallback_codepage` → windows-1252. DOS code pages that `encoding_rs` lacks
(437, 850, ...) fall through to the next candidate and raise a `dxf.codepage` warning. A cheap pre-scan of the
HEADER finds `$ACADVER` / `$DWGCODEPAGE` before the real pass.

## Reader mapping

### Document level

* `source.version` = `$ACADVER`, `source.codepage` = `$DWGCODEPAGE`, `source.application` = first `999` comment.
* `units` from `$INSUNITS` (21 → `UsSurveyFoot`; 22-24 → `Custom` with `meters_per_unit`).
* Every other header variable is kept as props `dxf.$NAME` (single value, or a list for points) — including
  `$EXTMIN` / `$EXTMAX`.
* LAYER → `Layer` (negative color = `visible: false`; flags 1 → frozen, 4 → locked; 290 → plottable;
  370 → lineweight; 420 → `Color::Rgb`; `dxf.flags`, xdata in props). LTYPE → `Linetype.pattern` (group 49
  values; complex-linetype elements are not modelled). STYLE → `TextStyle` (oblique in radians, bigfont and
  flags in props). BLOCK_RECORD feeds the handle → block-name map only.
* `BYLAYER` linetype → `None`; `BYBLOCK` → `Some("ByBlock")`.
* BLOCKS → `Block` (`id` = handle, `is_xref` = flag 4, `dxf.block_flags`, `dxf.layer`). `*Model_Space`,
  `*Paper_Space[N]` (and the R12 spellings `$MODEL_SPACE` / `$PAPER_SPACE`) are not blocks: their entities go to
  the model / layout.
* Spaces: model space is `models[0]` (`ModelKind::Model`, name `Model`). Layouts come from `LAYOUT` objects
  (name = group 1 of the `AcDbLayout` part, tab order 71, block record = last 330), sorted by tab order. An
  entity belongs to the space named by its owner handle (330 → BLOCK_RECORD name), else by group 67. Paper-space
  entities without a LAYOUT object (R12, stripped files) form one layout named `Paper Space` (R12) / `Layout1`.
* IMAGE → `Image`; the path comes from the `IMAGEDEF` object named by group 340 (resolved after OBJECTS).
* `GROUP` objects, `DICTIONARY` content, plot settings and the CLASSES section are not read.

### Coordinates (binding model rule)

The model has no OCS. Every OCS point/vector is converted to world coordinates with the arbitrary-axis algorithm
(`cadkit_core::geom_ops::arbitrary_axes`). Planar entities keep `normal`; angles stay relative to the
arbitrary-axis X of that normal.

| Entity | Stored in DXF | In the model |
|---|---|---|
| CIRCLE, ARC, TEXT, INSERT, HATCH, ATTRIB, SOLID/TRACE, LWPOLYLINE, 2D POLYLINE, DIMENSION 11/12/16 | OCS | world; `normal` kept (polylines: `Polyline.normal`, +Z for 3D polylines) |
| LINE, POINT, ELLIPSE, SPLINE, MTEXT, LEADER, 3DFACE, 3D POLYLINE, meshes, IMAGE, VIEWPORT, DIMENSION 10/13-15 | WCS | unchanged |

SOLID/TRACE corners are stored in "Z" order (1, 2, 4, 3); the model holds outline order, so corners 3/4 are
swapped on read and swapped back on write. A triangle repeats corner 3 in DXF.

### Entities

| DXF | Model | Notes |
|---|---|---|
| POINT, LINE, CIRCLE, ARC, ELLIPSE | same | angles degrees → radians |
| LWPOLYLINE | `Polyline` | bulge 42, widths 40/41/43, elevation 38, flags 1/128; `dxf.plinegen` |
| POLYLINE + VERTEX + SEQEND | `Polyline` (2D/3D), `Mesh` (polyface: flag 64; polygon mesh: flag 16, M×N quads, wrap flags 1/32) | spline-frame vertices (flag 16) are skipped; `dxf.polyline_3d`, `dxf.polyline_fit`, `dxf.polyface`, `dxf.polygon_mesh` |
| SPLINE | `Spline` | closed = flag 1 or 2; `dxf.spline.flags` |
| TEXT | `Text` | `position` = group 10 for Left/Baseline and Aligned/Fit text, group 11 (justification point) otherwise; `end_point` = group 11 for Aligned/Fit. Group 10 of justified text is not kept; the writer emits the justification point as both 10 and 11 |
| MTEXT | `MText` | `value` = chunks (3…) + 1; `plain` strips `\P \~ \\ \{ \} \L \l \O \o \K \k`, parameter codes up to `;`, stacking `\S` → `a/b`, `%%d %%p %%c`; rotation from group 11 when present |
| INSERT (+ATTRIB) | `Insert` + `attributes` | columns/rows/spacings from 70/71/44/45 |
| ATTDEF | `Unknown { "dxf.ATTDEF" }` | props `dxf.attdef.{tag,prompt,default,position,height,flags,style}`; no warning |
| HATCH | `Hatch` | all edge types and polyline paths; `dxf.hatch.{pattern_lines,seeds,style,pattern_type,associative}`; gradient data is not read |
| DIMENSION | `Dimension` | `points` in the order of props `dxf.dim.roles` (group codes 10, 13, 14, 15, 16, 12); `dxf.dim.{flags,rotation,oblique,text_rotation,attachment,line_spacing}`; empty text → `None` |
| SOLID, TRACE, 3DFACE | `Face` | `dxf.type`, `dxf.invisible_edges` |
| LEADER | `Leader` | `dxf.leader.{dimstyle,path_type}` |
| IMAGE | `Image` | u/v vectors scaled to the full image edge; `dxf.imagedef` |
| VIEWPORT | `Viewport` | `dxf.viewport.{id,status,view_direction,view_target,twist}` |
| others (3DSOLID, MESH, MLINE, MULTILEADER, SHAPE, RAY, XLINE, REGION, TABLE, WIPEOUT, TOLERANCE, …) | `Unknown { "dxf.<NAME>" }` | props `dxf.codes` (list of `[code, value]`); one `dxf.unknown_entity` warning per type with the occurrence count |

Common groups: handle 5 → `id`, layer 8 (missing → `"0"`), linetype 6, color 62 / 420, lineweight 370, visibility
60, `dxf.thickness` (39), `dxf.linetype_scale` (48), XDATA → props `dxf.xdata.<APPID>` as a list of
`[code, value]` lists. Reactor blocks (`102 {…` … `102 }`) are dropped.

### Limits

`max_input_bytes` (input), `max_string_bytes` (every string, checked before decoding), `max_objects` (entity
count), `max_vertices` (points / knots / weights / mesh faces / sub-records per entity; also bounds the groups per
record at `min(8 × max_vertices + 4096, 8 000 000, input bytes / 2 + 16)`: 8 M groups is about 320 MB of pairs, well above any real record). MTEXT stripping and the reader have no recursion (blocks are not expanded),
so `max_depth` is not needed; the writer bounds group nesting at 64. Exceeding a limit fails with
`Error::LimitExceeded`; a damaged or truncated file still yields a partial document with a `dxf.truncated`
warning when at least one section was read.

## Writer

Common to all versions: CRLF line ends, group codes right-aligned in 3 columns, doubles written with the
shortest round-trip representation (`{}` / `{:e}` for very large or small values), control characters as `^X`,
literal `^` as `^ `. Strings are UTF-8 for R2007+; older versions escape every non-ASCII character as
`\U+XXXX` (UTF-16 code units). Names are sanitised (`< > / \ " : ; ? * | , = `` ` `` and control characters →
`_`; a leading `*` is kept) and de-duplicated case-insensitively; `Model` is reserved for layouts. Layers,
linetypes, text styles and dimension styles that are referenced but undefined are created.

* **R12** — HEADER (`$ACADVER` …), TABLES (LTYPE, LAYER, STYLE, APPID, DIMSTYLE), BLOCKS, ENTITIES. No handles,
  subclass markers, 370/420/60, `$INSUNITS`. LWPOLYLINE → POLYLINE/VERTEX/SEQEND; MTEXT → one TEXT per line;
  ELLIPSE and SPLINE → sampled 3D POLYLINE; HATCH → its boundary as LINE/ARC/POLYLINE entities; LEADER →
  POLYLINE; IMAGE is skipped. All layouts merge into one paper space (`67 1`). VIEWPORT carries the `ACAD` /
  `MVIEW` xdata block that R12 readers (ezdxf) require. True colors map to the nearest ACI.
* **R2000+** — fresh handles for everything (model ids are not preserved), `$HANDSEED`, CLASSES
  (`LAYOUT`, `ACDBDICTIONARYWDFLT`, `ACDBPLACEHOLDER`, plus `IMAGE` classes when images exist), tables
  VPORT `*Active`, LTYPE ByBlock/ByLayer/Continuous, LAYER, STYLE Standard, VIEW, UCS, APPID, DIMSTYLE
  Standard, BLOCK_RECORD (`*Model_Space`, `*Paper_Space`, `*Paper_Space0…`, user blocks; R2007+ adds 70/280/281),
  BLOCKS (`*Model_Space`, `*Paper_Space*`, user blocks), ENTITIES (model space, then the first layout with
  `67 1`), OBJECTS (root dictionary, `ACAD_GROUP`, `ACAD_LAYOUT`, `ACAD_PLOTSETTINGS`, `ACAD_PLOTSTYLENAME` with
  the `Normal` placeholder, `ACAD_IMAGE_DICT`, one LAYOUT per space, IMAGEDEF + IMAGEDEF_REACTOR). Layouts after
  the first keep their entities inside their `*Paper_SpaceN` block, as AutoCAD does. Group 420 is written for
  R2004+; R2000 uses the nearest ACI. The dimension measurement (42) is written for R2007+.
* **Spaces** — `Model`s of kind `Model` after the first, and every `Layout`/`Sheet`, become layouts. A document
  without layouts gets an empty `Layout1`.
* **Groups** are flattened (children are written in place); the group name goes to XDATA
  `CADKIT_GROUP` (`1000 name`) on each child. `Group.origin` is dropped.
* **Unknown** entities are skipped with a `dxf.write.skipped` warning, except `dxf.ATTDEF` which is rebuilt from
  its props. INSERTs of undefined blocks, VIEWPORTs outside paper space, empty polylines and unusable
  splines/hatches are skipped with a warning.
* **Entity specifics** — ELLIPSE with ratio > 1 is rewritten with swapped axes; SPLINE without knots gets clamped
  uniform knots, fit-point-only splines are written without control points; MESH faces with more than four
  corners are fan-triangulated into a polyface; HATCH without stored pattern lines (`dxf.hatch.pattern_lines`)
  gets a generic 45° line family; DIMENSION of type ArcLength/Other is skipped; line weights snap to the standard
  AutoCAD values; non-planar or non-+Z entities are converted back to OCS with their `normal`.

## Verification (this session)

All numbers come from `cargo test -p cadkit-dxf` and the two Python scripts in `crates/cadkit-dxf/tests/`.

* ASCII vs binary, all 7 versions: identical documents (ids/handles, header GUIDs/timestamps and the raw
  `dxf.codes` of unknown entities excluded because the two files are separate saves). Max numeric deviation
  3.4e-15 (R2000+) and 4.6e-6 for AC1009, whose ASCII twin was exported with fewer digits (many coordinates
  have 1-4 decimals) than the binary one.
* Round trip sample → write (7 versions) → read: 49 round trips, entity-by-entity geometry within 1e-9.
* `validate_with_ezdxf.py`: all written files load strictly and via `recover`, 0 audit errors.
* `compare_reader_with_ezdxf.py`: cadkit and ezdxf agree on entity histograms of model space, paper space and
  every block, layer sets, block names and summed line length / circle radius / arc sweep for all samples.
* Robustness: 352 truncated / bit-flipped / overwritten corpus inputs plus noise and header damage, no panic.

## Known gaps

* Dimensions that were not read from a file with their anonymous block are written without group 2; ezdxf
  discards such DIMENSIONs on load (AutoCAD regenerates them).
* Unknown entity families (3DSOLID/REGION/BODY SAT data, MESH, MLINE, MULTILEADER, TABLE, WIPEOUT, SHAPE,
  OLE2FRAME, proxy entities) are kept only as `dxf.codes`; they are not written back.
* Complex-linetype elements (shape/text in LTYPE), hatch gradients, `GROUP` objects, DIMSTYLE variables and
  VIEWPORT frozen-layer lists are not modelled.
* Binary `\M+` code-page selector mapping is best effort; DOS code pages other than 866 are not decoded.
* Not tested against AutoCAD itself (no vendor software); the strictness reference is ezdxf's audit.

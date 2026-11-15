# DGN external acceptance protocol

Status (2026-11-15): **no external acceptance is claimed.** No file written by cadkit has
been opened by the cadkit maintainers in MicroStation or in any other DGN application.
Every statement about DGN output in this repository rests on local evidence: cadkit's own
reader, `check_structure` and the `cfb` crate's strict open
([FORMAT_NOTES](FORMAT_NOTES.md#evidence-and-acceptance-boundary),
[implementation status](../INTERCHANGE_STATUS.md)). This document is the protocol for the
person who closes that gap by hand: what to generate, what to open, what must be true, what
to record and what to send back. Until a filled results table and the saved copies exist,
the status above stands.

The pass has two sessions with the same kit: **A** in MicroStation (any V8-format version;
record which) and **B** in any other application that reads DGN V8 (record which). Session B
is optional and shorter. Screenshots are optional; the results table is not.

## 1. Generate the kit

From a checkout of this repository:

```sh
python3 scripts/fetch-corpus.py                          # public samples into corpus/public/
python3 scripts/buildlock.py cargo run -p cadkit --example interchange -- \
  corpus/public/gdal/test_dgnv8.dgn target/interchange-validation
python3 scripts/buildlock.py cargo build -p cadkit-cli   # target/debug/cadkit
```

The generator writes every file, reads it back with cadkit, compares it with what was
written (geometry within 1e-6 relative, levels, visibility, colour indexes, texts, tag sets,
tag values, tag levels, positions and rotations), requires a `check_structure` report
without warnings and a strict `cfb` open, and stops with an error naming the file and the
check otherwise. A kit that was written completely has passed those local checks; nothing
more. The integration test `crates/cadkit/tests/acceptance_kit.rs` runs the same generator
twice into a temporary directory and also requires byte-identical output.

The seed is GDAL's public `test_dgnv8.dgn` (MIT/X, an ODA-written 3D V8 file, metres). The
Safe Software samples are not used: they are for local testing only and nothing in the kit
is derived from them. Everything the kit adds (level, tag set and tag names, texts, tag
values, coordinates) is invented. The kit is not committed; regenerate it from the commands
above.

| File | What it exercises |
|---|---|
| `01-seed-repacked.dgn` | the seed in a new compound file (`repack_v8`), every stream unchanged |
| `02-seed-stripped.dgn` | the seed after `strip_seed_v8` (no graphic or control record, minimal property sets, tables kept) |
| `03-stripped-with-geometry.dgn` | geometry, a text and a hidden tag written into 02 |
| `04-geometry.dgn` | lines, a line string, a circle, an arc, an ellipse, a complex chain, a closed shape, a cell and a shape with a hole on three new levels with distinct colour indexes, and a line rising in Z |
| `05-text-utf16.dgn` | left-baseline, centre-middle, right-top and rotated text with Turkish letters (UTF-16), each with a cross at its justification point |
| `06-text-cp1254.dgn` | the texts of 05 and a displayed Turkish tag, written as 8-bit windows-1254 |
| `07-tags.dgn` | one tag set with text, integer and double tags: displayed unrotated, displayed at 30 degrees, hidden; tag elements on their own level |
| `08-hidden.dgn` | a hidden line (property bit `0x0080`) next to a visible one |
| `09-plan-in-3d.dgn` | a 2D document promoted to the 3D seed by `V8Seed::prepare_document` |
| `10-large-coordinates.dgn` | geometry at X 4,500,000, Y 4,400,000 (master units) |
| `11-raster.dgn` + `kit-raster.tif` | a new raster attachment (cadkit's built-in attachment group, the seed has none) naming a 64 x 32 1-bit TIFF beside the DGN without a folder |
| `12-chain-step1.dgn`, `13-chain-step2.dgn` | a write whose output is the seed of the next write (level and tag set reused, one new level) |
| `14-bylayer.dgn` | a ByLayer entity written with its layer's colour index next to an element storing the by-level colour value |
| `synthetic.gml` | CityGML only (`scripts/validate-citygml.py`); not part of this protocol |
| `MANIFEST.json`, `README.md` | per file: purpose, seed, write options, every element with its level, colour index and the RGB cadkit resolves, texts, tag values, the `check_structure` summary and the checks below |

The kit covers a 3D seed only. 2D-specific paths (2D tag angles, issue #29; text and tags in
a 2D model) need a 2D V8 seed whose use is permitted. A maintainer may run the generator with
their own 2D template as the seed; that kit then contains the template's tables and must not
be shared further than the template itself.

Before the session, record the cadkit baseline of every file (step 5 compares against it):

```sh
K=target/interchange-validation; CADKIT=target/debug/cadkit
mkdir -p "$K/baseline"
for f in "$K"/*.dgn; do
  n=$(basename "$f" .dgn)
  "$CADKIT" info --json "$f" > "$K/baseline/$n.info.json"
  "$CADKIT" check-dgn "$f" > "$K/baseline/$n.check.json"
  "$CADKIT" layers "$f" > "$K/baseline/$n.layers.txt"
  "$CADKIT" --codepage windows-1254 dump "$f" | cut -f2- > "$K/baseline/$n.dump.txt"
  "$CADKIT" --codepage windows-1254 dump --json "$f" > "$K/baseline/$n.json"
done
```

`dump` prints one line per element (id, level, kind, geometry summary); `cut -f2-` drops the
element id, which an application may renumber. `--codepage windows-1254` matters only for
06, whose 8-bit tag value cadkit decodes correctly only with it (without it the value reads
as windows-1252); the other files store UTF-16.

## 2. Open and check (sessions A and B)

Copy the whole kit directory to the machine with the application; keep `kit-raster.tif`
beside `11-raster.dgn`. Record the application name, version and build, the operating
system, and any setting that changes how DGN files are read (workspace or configuration,
code page or locale, raster search paths). Open each file on its own, from the kit
directory, without a reference attachment.

For **every** file, in addition to its own checks:

- **G1** Does it open without an error, a warning, a repair, upgrade or recovery prompt?
  Record any message verbatim.
- **G2** "Fit view" shows the expected elements and nothing else (the manifest lists them;
  01 shows the seed's drawing).
- **G3** The level list holds the seed's level and the levels the manifest names, each once.
- **G4** Element information of one element per level shows the level and colour index the
  manifest names.

File-specific checks (the same text is in the kit's `README.md`, per file, as a checklist):

| Check | File | Must be true / record |
|---|---|---|
| 01.1 | 01 | Same elements, levels and views as the seed opened beside it (`corpus/public/gdal/test_dgnv8.dgn`). |
| 02.1 | 02 | Opens as an empty drawing; the seed's level, font and other tables are listed as before. |
| 02.2 | 02 | Draw one line on the seed's level, save, close. Record any prompt. |
| 03.1 | 03 | Level `Kit Stripped`: a closed 10 x 10 square at the origin, its diagonal, the text `Stripped seed`. |
| 03.2 | 03 | The square carries tag set `Kit Stripped Tags`, tag `Note` = `from the stripped seed`, not displayed but listed in the tag dialog. |
| 04.1 | 04 | Levels `Kit Lines` (index 1), `Kit Curves` (index 2), `Kit Groups` (index 4); the components of the chain and the cells are on `Kit Groups` too. Record the colour each index shows. |
| 04.2 | 04 | Element types as listed in the manifest: 3 lines and a line string, a circle, an arc (0 to 90 degrees), an ellipse (semi-axes 5 and 2.5), a complex chain (line, half circle, line), a closed triangle, cell `KITCELL` (square and circle), and an unnamed cell of two shapes, the inner flagged as a hole. |
| 04.3 | 04 | With fill on, is the inner area of the holed cell empty (a hole) or filled (two plain shapes)? |
| 04.4 | 04 | The line at Y 40 rises from Z 0 to Z 5 (side view or element information). |
| 05.1 | 05 | The four texts read exactly `Sol taban ÇĞİÖŞÜ`, `Orta çğıöşü`, `Sağ üst İğne`, `Döndürülmüş 30°`. |
| 05.2 | 05 | Each text's justification (left-baseline, centre-middle, right-top, left-baseline) is reported as written and its justification point lies on its red cross. |
| 05.3 | 05 | The fourth text runs at 30 degrees counter-clockwise. |
| 05.4 | 05 | Centre and right texts were written with an invented text length (0.8 x height per character, `dgn.text_length`): record the offset between the drawn text and its cross along the baseline, if any. cadkit measures no fonts. |
| 06.1 | 06 | The four texts read as in 05.1. |
| 06.2 | 06 | The square on `Kit Text Tags` displays tag `Açıklama` = `Işık şiddeti ölçümü` (set `Kit Türkçe`). Record whether the letters are right and whether a code page setting was needed. |
| 07.1 | 07 | Tag set `Kit Tag Set` with `Label` (text), `Count` (integer), `Length` (double). |
| 07.2 | 07 | Square A shows `Kapı A`, `42`, `12.5` unrotated above it; square B shows `Pencere B`, `-7`, `0.25` rotated 30 degrees; square C shows nothing, and its values `Gizli C`, `2147483647`, `1234.125` are in the tag dialog. |
| 07.3 | 07 | Every tag element is on level `Kit Tags` (colour index 5); switching that level off hides the displayed values and not the squares (`Kit Tag Owners`). |
| 08.1 | 08 | The line under `VISIBLE` is drawn; nothing is drawn under `HIDDEN`. |
| 08.2 | 08 | Can the hidden line be found (select all, element information, a "show hidden" display setting)? Record what the application reports. |
| 09.1 | 09 | A square, a circle and the text `Plan in 3D` at Z 0 on `Kit Plan`; flat in a side view. |
| 10.1 | 10 | The square's lower-left corner reads X 4,500,000, Y 4,400,000 (Z 5) in master units; record any design-plane or range warning. |
| 11.1 | 11 | The raster manager (or equivalent) lists one attachment `kit-raster.tif`. Record whether the application found the image from the DGN's folder without asking: cadkit stores the bare name and an empty full path. |
| 11.2 | 11 | The image fills the outline on `Kit Raster Frame`: its black border on the outline, the solid black square in the top-left corner (64 x 32 pixels, 0.5 master units each). Record the display mode (bitonal, colours). |
| 12.1 | 12 | Level `Kit Chain`: one line with hidden tag `Step` = 1 (set `Kit Chain Tags`). |
| 13.1 | 13 | Level `Kit Chain`: a circle with `Step` = 2; level `Kit Chain Two`: a line with `Step` = 3; step 1's line is gone. `Kit Chain` and `Kit Chain Tags` exist once each. |
| 14.1 | 14 | Record the colour the level manager shows for `Kit ByLayer`. cadkit writes no level colour: a new level copies the seed's first level entry, whose colour field cadkit does not decode for this seed. |
| 14.2 | 14 | The line labelled `materialized` shows colour index 6 (element information: 6, not "by level"). |
| 14.3 | 14 | The line labelled `by level` is drawn in the level's colour and element information shows "by level" or the application's equivalent. |
| 14.4 | 14 | Change the level's colour: record which line follows. Undo the change before saving. |

## 3. Save and send back

For every file that opened:

1. Save a copy in the application's own V8 DGN format, under the same file name, into a
   folder `saved/`, without editing (02 is the exception: save it after check 02.2, and also
   save an unedited copy as `saved/02-seed-stripped-unedited.dgn`). If the application
   writes changes to the open file directly, close it and copy the file. Do not save over the
   kit files.
2. Note for each save whether the application asked anything (format upgrade, compression,
   code page) and what was answered.
3. Send back `saved/` and the filled results table (section 6), and the application's own
   log or message window text if any message appeared. Screenshots are optional.

The saved copies hold only invented content and the public GDAL seed's tables. Whether any
of them may be committed as a fixture is a maintainer decision (the application's terms may
matter); until then they stay outside the repository like the rest of the corpus.

## 4. Session B: another DGN application

Repeat sections 2 and 3 with the other application. The general checks G1–G4 and checks
04.1–04.3, 05.1–05.3, 06.1–06.2, 07.1–07.2, 08.1, 10.1, 11.1–11.2 and 13.1 apply as far as
the application shows that information; mark the others "n/a" rather than guessing.
Record whether the application reads tags, raster attachments and hidden elements at all.

## 5. Compare the saved copies with cadkit

```sh
K=target/interchange-validation; S=path/to/saved; CADKIT=target/debug/cadkit
for f in "$S"/*.dgn; do
  n=$(basename "$f" .dgn)
  echo "== $n"
  "$CADKIT" check-dgn "$f" > "$S/$n.check.json"; echo "check-dgn exit $?"
  "$CADKIT" info --json "$f" | diff "$K/baseline/$n.info.json" - 
  "$CADKIT" layers "$f" | diff "$K/baseline/$n.layers.txt" -
  "$CADKIT" --codepage windows-1254 dump "$f" | cut -f2- | diff "$K/baseline/$n.dump.txt" -
done
```

(`02-seed-stripped-unedited` has no baseline of its own; diff it against
`02-seed-stripped`.) For tag values, list every tag of a file with

```sh
"$CADKIT" --codepage windows-1254 dump --json FILE | python3 -I -c '
import json, sys
d = json.load(sys.stdin)
for m in d["models"]:
    for e in m["entities"]:
        for a in e.get("attributes", []):
            print(e.get("layer"), a.get("set"), a["tag"], json.dumps(a["value"], ensure_ascii=False),
                  "hidden" if a.get("invisible") else "displayed", sep="\t")'
```

and diff the output for the kit file and the saved copy. Read the results as follows:

- `check-dgn` exit 1 on a saved copy is information about the application's writer, not a
  defect of the copy: two of the three public V8 samples (test_dgnv8, Water) report
  `dgn.cfb_free_entry`, and MS-CFB permits unbalanced sibling trees
  (`dgn.cfb_tree_unbalanced`). `dgn.model_page_counter` or `dgn.record_time` on a saved copy
  would contradict an observed relation ("Model header", "Record times") and must be
  recorded in FORMAT_NOTES.
- A changed element count, level, colour index, text or tag value between the kit file and
  its unedited saved copy means the application read something differently from what
  cadkit wrote (or rewrote it on save). Record the difference with the check it belongs to.
- cadkit must read every saved copy without an error. A reader warning or an `Unknown`
  element in a saved copy is a reader gap to report as an issue, with structural facts only.
- Use a saved copy as a seed once: `cadkit strip-seed saved/04-geometry.dgn /tmp/seed.dgn
  --report` and `cadkit check-dgn /tmp/seed.dgn`, then the Rust host contract
  (`write_v8` with `clear_seed_model`) on it. This shows whether cadkit accepts the
  application's own output as a seed.

Stream-level differences (which streams and records the application added or dropped) need
the native API, for example:

```rust
let bytes = std::fs::read("saved/02-seed-stripped.dgn")?;
let cf = cadkit::dgn::cfb::CompoundFile::parse(&bytes)?;
for p in cf.paths() {
    println!("{:?}\t{}", p.kind, p.path);
}
let native = cadkit::dgn::native::v8::read(&bytes, &cadkit::ReadOptions::default())?;
for m in &native.models {
    for page in m.graphic_pages.iter().chain(&m.control_pages) {
        let types: Vec<u16> = page.elements.iter().map(|e| e.type_code()).collect();
        println!("{} {:?}", page.path, types);
    }
}
```

Record findings in [FORMAT_NOTES](FORMAT_NOTES.md) with the file and the application as the
evidence ("observed in a MicroStation <version> save of kit file 02"), and update
[implementation status](../INTERCHANGE_STATUS.md#external-acceptance-and-limitations).

## 6. Results table

One row per file and application. Copy into the reply, or fill the kit's `README.md`
checkboxes and add this table.

| File | Application, version | G1 opens (message) | G2–G4 | File checks passed | File checks failed or deviating (what was seen) | Saved copy | Save prompt / answer | Notes |
|---|---|---|---|---|---|---|---|---|
| 01 | | | | | | | | |
| 02 | | | | | | | | |
| 03 | | | | | | | | |
| 04 | | | | | | | | |
| 05 | | | | | | | | |
| 06 | | | | | | | | |
| 07 | | | | | | | | |
| 08 | | | | | | | | |
| 09 | | | | | | | | |
| 10 | | | | | | | | |
| 11 | | | | | | | | |
| 12 | | | | | | | | |
| 13 | | | | | | | | |
| 14 | | | | | | | | |

## 7. Open questions this pass can settle

Each item names the open question in [FORMAT_NOTES](FORMAT_NOTES.md) and the observation that
would settle it. An observation from one application version settles it for that version
only; record the version.

| Question | Where it stands | Observation that settles it |
|---|---|---|
| Which removed records an application needs from a stripped seed: the model settings control records 66 / 100, the locale property, the dropped empty storages `Dgn^C`, `Dgn^GA`, `Dgn^CA` ("Seed stripping", Open) | stripped seeds are clean locally; never opened elsewhere | 02 and 03 open without a prompt (G1) and 02.2 saves: the record types of the saved 02 (native API above) show what the application re-creates; a prompt or a refusal names what is missing. Compare with the unedited saved 02. |
| Which kept records an application relies on (dictionary records, `Dgn~S`, `Dgn~Mf`, the model index entry tail are kept unread) | kept byte for byte | The same saves: kept streams that come back changed (diff of the stream listing and sizes) show which ones the application rewrites. |
| Model header page counter, first dword of the `Dgn~Mh` prefix (open question 14, "Model header") | an observed relation (552 of 567 private and 3 of 3 public files); cadkit writes the number of pages holding records | Every kit file opens (G1), and the saved copies report no `dgn.model_page_counter`. A prompt on open, or a saved copy whose counter differs from its filled pages, would contradict the relation. |
| Free directory entries ("Compound file") | cadkit writes every unused slot in the MS-CFB 2.6.3 form; the seed's own unused slots are not in that form | 01 opens like the seed (G1, 01.1); `irregular_free_entries` in the `check-dgn` output of the saved copies shows the application's own form. |
| ByLayer colour ("Level colors", "Level sentinels"; open question 1 for level symbology) | ByLayer is written as the layer's colour index (materialized), the sentinel `0xFFFFFFFF` as stored; new levels copy the seed's first level entry | 14.1–14.4: the level's displayed colour, which line follows a level colour change, and whether the saved copy keeps the sentinel and the index 6. |
| Fields blocking a built-in blank seed (open question 17, "Built-in blank seed") | nine `Dgn~H` dwords, model header prefix `+4` / `+8` and model header fields differ between writers; meaning unknown | The unedited saved copies of 01 and 02 (and a save of 02 with one setting changed, e.g. the active level, saved as `saved/02-seed-stripped-one-change.dgn`) give values the application writes for an ODA-written 3D seed; cadkit compares them with `tests/blank_seed_evidence.rs`. This adds data points; only a stable value with a stated meaning settles a field. |
| Hidden graphic elements (open question 8) | bit `0x0080` from one sample's tags and an external report | 08.1–08.2, and whether the saved 08 keeps the bit. |
| Text length estimate and range rule (open question 9) | cadkit measures no fonts | 05.4 (offset of centre and right texts) and the stored lengths in the saved 05 (`dgn.text_length` in `dump --json`). |
| Relative raster paths and the built-in attachment group ("Raster attachments") | display of the built-in group and resolution of a bare name from the DGN's folder not confirmed | 11.1–11.2, and the full path the saved 11 stores. |
| Code page of 8-bit tag text ("Strings") | cadkit decodes 8-bit tags with the fallback code page | 06.2, and whether the saved 06 stores the tag as 8-bit or UTF-16. |

Questions this kit does not reach: level flags (open question 1 beyond colour), B-spline
closure (2), extended colours (3), sheet models (4), storage units (5), unmodelled element
types (6), V7 tags (7), complex counts (11), byte-order-marked names (12), custom line
styles (13), repeated level ids (15) and the unknown property bits (16). They need files with
those features, as FORMAT_NOTES states for each.

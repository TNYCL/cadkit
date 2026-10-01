//! ASCII DXF writer.
//!
//! R12 output is a minimal valid file (HEADER, TABLES, BLOCKS, ENTITIES). R2000+ output also
//! carries handles, the full required table set, `*Model_Space` / `*Paper_Space` blocks and an
//! OBJECTS section with the root dictionary, `ACAD_GROUP`, `ACAD_LAYOUT` and one LAYOUT per
//! space. The structure follows what AutoCAD-compatible readers expect (cross-checked against
//! files produced by ezdxf, MIT); see `docs/dxf/NOTES.md`.

mod curves;
mod entities;
mod out;

use std::collections::{BTreeSet, HashMap};

use cadkit_core::{
    Color, Document, Entity, EntityKind, Lineweight, ModelKind, Point3, Result, Warning,
};

use self::out::Out;
use crate::DxfVersion;

/// Case-insensitive name registry that remembers the canonical spelling.
#[derive(Default)]
pub(crate) struct Names {
    map: HashMap<String, String>,
}

impl Names {
    /// Registers `name` (sanitised) and returns the canonical spelling.
    pub fn add(&mut self, name: &str) -> String {
        let clean = clean_name(name);
        self.map
            .entry(clean.to_lowercase())
            .or_insert(clean)
            .clone()
    }

    pub fn get(&self, name: &str) -> Option<&String> {
        self.map.get(&clean_name(name).to_lowercase())
    }
}

/// Replaces characters DXF forbids in table names (`< > / \ " : ; ? * | , = ` and control
/// characters) with `_` and limits the length. A leading `*` is kept (anonymous blocks).
pub(crate) fn clean_name(name: &str) -> String {
    let mut s: String = name
        .chars()
        .enumerate()
        .map(|(i, c)| {
            if "<>/\\\":;?|,=`".contains(c) || (c == '*' && i > 0) || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    if s.chars().count() > 255 {
        s = s.chars().take(255).collect();
    }
    let t = s.trim();
    if t.is_empty() {
        "_".to_owned()
    } else {
        t.to_owned()
    }
}

/// Hands out case-insensitively unique names (`base`, `base_2`, `base_3`, ...) in O(1)
/// amortised time per request.
#[derive(Default)]
pub(crate) struct UniqueNames {
    used: std::collections::HashSet<String>,
    next: HashMap<String, u64>,
}

impl UniqueNames {
    pub fn reserve(&mut self, name: &str) {
        self.used.insert(name.to_lowercase());
    }

    pub fn make(&mut self, base: &str) -> String {
        let key = base.to_lowercase();
        if self.used.insert(key.clone()) {
            return base.to_owned();
        }
        let counter = self.next.entry(key).or_insert(2);
        loop {
            let name = format!("{base}_{counter}");
            *counter += 1;
            if self.used.insert(name.to_lowercase()) {
                return name;
            }
        }
    }
}

/// Visits every entity, descending into groups.
pub(crate) fn walk<'e>(entities: &'e [Entity], f: &mut dyn FnMut(&'e Entity), depth: u32) {
    for e in entities {
        f(e);
        if let EntityKind::Group { children, .. } = &e.kind {
            if depth < 64 {
                walk(children, f, depth + 1);
            }
        }
    }
}

/// One drawing space to write.
struct Space<'a> {
    name: String,
    entities: &'a [Entity],
    block_name: String,
    block_record: u64,
    layout: u64,
}

pub(crate) struct Writer<'a> {
    pub doc: &'a Document,
    pub ver: DxfVersion,
    pub r12: bool,
    pub ge2004: bool,
    pub ge2007: bool,
    pub next_handle: u64,
    pub warnings: Vec<Warning>,
    pub layers: Names,
    pub linetypes: Names,
    pub styles: Names,
    pub dimstyles: Names,
    pub blocks: Names,
    pub appids: BTreeSet<String>,
    pub images: Vec<entities::ImageUse>,
    pub viewport_id: i64,
    /// Spelling of the CONTINUOUS linetype (the document's own when it defines one).
    pub continuous: String,
    /// Handle of the `Normal` plot style placeholder (R2000+; 0 until allocated).
    pub plotstyle: u64,
}

pub(crate) fn write(doc: &Document, ver: DxfVersion) -> Result<(String, Vec<Warning>)> {
    let mut w = Writer {
        doc,
        ver,
        r12: ver == DxfVersion::R12,
        ge2004: !matches!(ver, DxfVersion::R12 | DxfVersion::R2000),
        ge2007: matches!(
            ver,
            DxfVersion::R2007 | DxfVersion::R2010 | DxfVersion::R2013 | DxfVersion::R2018
        ),
        next_handle: 1,
        warnings: Vec::new(),
        layers: Names::default(),
        linetypes: Names::default(),
        styles: Names::default(),
        dimstyles: Names::default(),
        blocks: Names::default(),
        appids: BTreeSet::new(),
        images: Vec::new(),
        viewport_id: 1,
        plotstyle: 0,
        continuous: if ver == DxfVersion::R12 {
            "CONTINUOUS"
        } else {
            "Continuous"
        }
        .to_owned(),
    };
    let text = w.run();
    Ok((text, w.warnings))
}

impl Writer<'_> {
    pub fn new_handle(&mut self) -> u64 {
        let h = self.next_handle;
        self.next_handle += 1;
        h
    }

    pub fn warn(&mut self, code: &str, message: String) {
        self.warnings.push(Warning {
            code: code.to_owned(),
            message,
            offset: None,
            object: None,
        });
    }

    pub fn out(&self) -> Out {
        Out::new(self.ge2007)
    }

    /// Canonical linetype name; `None` for ByLayer.
    pub fn linetype_name(&mut self, name: &str) -> Option<String> {
        let lower = name.trim().to_lowercase();
        match lower.as_str() {
            "" | "bylayer" => None,
            "byblock" => Some("ByBlock".to_owned()),
            "continuous" => Some(self.continuous.clone()),
            _ => Some(self.linetypes.add(name)),
        }
    }

    fn collect_references(&mut self) {
        let doc = self.doc;
        if let Some(lt) = doc
            .linetypes
            .iter()
            .find(|l| l.name.trim().eq_ignore_ascii_case("continuous"))
        {
            self.continuous = clean_name(&lt.name);
        } else if let Some(name) = doc
            .models
            .iter()
            .flat_map(|m| m.entities.iter())
            .chain(doc.blocks.iter().flat_map(|b| b.entities.iter()))
            .filter_map(|e| e.linetype.as_deref())
            .chain(doc.layers.iter().filter_map(|l| l.linetype.as_deref()))
            .find(|n| n.trim().eq_ignore_ascii_case("continuous"))
        {
            self.continuous = clean_name(name);
        }
        self.layers.add("0");
        for l in &doc.layers {
            self.layers.add(&l.name);
            if let Some(lt) = &l.linetype {
                self.linetype_name(lt);
            }
            for k in l.props.keys() {
                if let Some(app) = k.strip_prefix("dxf.xdata.") {
                    self.appids.insert(clean_name(app));
                }
            }
        }
        for lt in &doc.linetypes {
            self.linetype_name(&lt.name);
        }
        self.styles.add("Standard");
        for s in &doc.text_styles {
            self.styles.add(&s.name);
        }
        self.dimstyles.add("Standard");
        for b in &doc.blocks {
            let n = clean_name(&b.name);
            let lower = n.to_lowercase();
            if lower == "*model_space" || lower.starts_with("*paper_space") {
                continue;
            }
            self.blocks.add(&b.name);
        }
        let mut seen: Vec<&Entity> = Vec::new();
        let all = doc
            .models
            .iter()
            .map(|m| m.entities.as_slice())
            .chain(doc.blocks.iter().map(|b| b.entities.as_slice()));
        for list in all {
            walk(list, &mut |e| seen.push(e), 0);
        }
        for e in seen {
            self.layers.add(e.layer.as_deref().unwrap_or("0"));
            if let Some(lt) = &e.linetype {
                self.linetype_name(lt);
            }
            for k in e.props.keys() {
                if let Some(app) = k.strip_prefix("dxf.xdata.") {
                    self.appids.insert(clean_name(app));
                }
            }
            match &e.kind {
                EntityKind::Text { style: Some(s), .. }
                | EntityKind::MText { style: Some(s), .. } => {
                    if !s.trim().is_empty() {
                        self.styles.add(s);
                    }
                }
                EntityKind::Dimension { style, .. } => {
                    if let Some(s) = style.as_ref().filter(|s| !s.trim().is_empty()) {
                        self.dimstyles.add(s);
                    }
                }
                EntityKind::Group { name: Some(_), .. } => {
                    self.appids.insert(entities::GROUP_APPID.to_owned());
                }
                _ => {}
            }
        }
        for b in &doc.blocks {
            for k in b.props.keys() {
                if let Some(app) = k.strip_prefix("dxf.xdata.") {
                    self.appids.insert(clean_name(app));
                }
            }
        }
    }

    fn plan_spaces<'d>(&mut self, doc: &'d Document) -> (Space<'d>, Vec<Space<'d>>) {
        let model_idx = doc.models.iter().position(|m| m.kind == ModelKind::Model);
        let model_entities: &[Entity] = model_idx
            .and_then(|i| doc.models.get(i))
            .map_or(&[], |m| m.entities.as_slice());
        let model = Space {
            name: "Model".to_owned(),
            entities: model_entities,
            block_name: "*Model_Space".to_owned(),
            block_record: 0,
            layout: 0,
        };
        let mut layouts: Vec<Space<'d>> = Vec::new();
        let mut used = UniqueNames::default();
        used.reserve("model");
        for (i, m) in doc.models.iter().enumerate() {
            if Some(i) == model_idx {
                continue;
            }
            let base = clean_name(&m.name);
            let name = used.make(&base);
            layouts.push(Space {
                name,
                entities: &m.entities,
                block_name: String::new(),
                block_record: 0,
                layout: 0,
            });
        }
        if layouts.is_empty() {
            layouts.push(Space {
                name: "Layout1".to_owned(),
                entities: &[],
                block_name: String::new(),
                block_record: 0,
                layout: 0,
            });
        }
        for (i, l) in layouts.iter_mut().enumerate() {
            l.block_name = if i == 0 {
                "*Paper_Space".to_owned()
            } else {
                format!("*Paper_Space{}", i - 1)
            };
        }
        (model, layouts)
    }

    fn run(&mut self) -> String {
        let doc = self.doc;
        self.collect_references();
        let (mut model, mut layouts) = self.plan_spaces(doc);

        // Handles that are referenced before they are written.
        let r12 = self.r12;
        let mut doc_blocks: Vec<(String, u64, &cadkit_core::Block)> = Vec::new();
        let mut seen_blocks: BTreeSet<String> = BTreeSet::new();
        let mut root_dict = 0;
        let mut group_dict = 0;
        let mut layout_dict = 0;
        if !r12 {
            model.block_record = self.new_handle();
            model.layout = self.new_handle();
            for l in layouts.iter_mut() {
                l.block_record = self.new_handle();
                l.layout = self.new_handle();
            }
            root_dict = self.new_handle();
            group_dict = self.new_handle();
            layout_dict = self.new_handle();
            self.plotstyle = self.new_handle();
        }
        for b in &doc.blocks {
            let Some(name) = self.blocks.get(&b.name).cloned() else {
                continue;
            };
            if !seen_blocks.insert(name.to_lowercase()) {
                self.warn(
                    "dxf.write.duplicate_block",
                    format!("block {name} defined twice; the first definition is kept"),
                );
                continue;
            }
            let h = if r12 { 0 } else { self.new_handle() };
            doc_blocks.push((name, h, b));
        }

        // ---- ENTITIES (also discovers images) ----
        let mut entities_out = self.out();
        entities_out.word(0, "SECTION");
        entities_out.word(2, "ENTITIES");
        let model_owner = model.block_record;
        let model_entities = model.entities;
        self.write_entity_list(&mut entities_out, model_entities, model_owner, false);
        for (i, l) in layouts.iter().enumerate() {
            if r12 || i == 0 {
                self.write_entity_list(&mut entities_out, l.entities, l.block_record, true);
            }
        }
        entities_out.word(0, "ENDSEC");

        // ---- BLOCKS ----
        let mut blocks_out = self.out();
        blocks_out.word(0, "SECTION");
        blocks_out.word(2, "BLOCKS");
        if !r12 {
            self.write_block_shell(
                &mut blocks_out,
                "*Model_Space",
                model.block_record,
                Point3::default(),
                0,
                None,
            );
            self.end_block(&mut blocks_out, model.block_record);
            for (i, l) in layouts.iter().enumerate() {
                self.write_block_shell(
                    &mut blocks_out,
                    &l.block_name,
                    l.block_record,
                    Point3::default(),
                    0,
                    None,
                );
                if i > 0 {
                    self.write_entity_list(&mut blocks_out, l.entities, l.block_record, true);
                }
                self.end_block(&mut blocks_out, l.block_record);
            }
        }
        for (name, h, b) in &doc_blocks {
            let mut flags = 0;
            if name.starts_with('*') {
                flags |= 1;
            }
            let mut has_attdef = false;
            walk(
                &b.entities,
                &mut |e| {
                    if matches!(&e.kind, EntityKind::Unknown { type_name } if type_name == "dxf.ATTDEF")
                    {
                        has_attdef = true;
                    }
                },
                0,
            );
            if has_attdef {
                flags |= 2;
            }
            let xref = if b.is_xref {
                b.xref_path.as_deref()
            } else {
                None
            };
            if b.is_xref {
                flags |= 4;
            }
            self.write_block_shell(&mut blocks_out, name, *h, b.base_point, flags, xref);
            if !b.is_xref {
                self.write_entity_list(&mut blocks_out, &b.entities, *h, false);
            }
            self.end_block(&mut blocks_out, *h);
        }
        blocks_out.word(0, "ENDSEC");

        // ---- TABLES ----
        let tables_out = self.write_tables(&model, &layouts, &doc_blocks);

        // ---- OBJECTS ----
        let objects_out = if r12 {
            None
        } else {
            Some(self.write_objects(&model, &layouts, root_dict, group_dict, layout_dict))
        };

        // ---- HEADER / CLASSES ----
        let mut head = self.out();
        self.write_header(&mut head);
        let classes = if r12 {
            None
        } else {
            Some(self.write_classes())
        };

        let mut all = head.buf;
        if let Some(c) = classes {
            all.push_str(&c.buf);
        }
        all.push_str(&tables_out.buf);
        all.push_str(&blocks_out.buf);
        all.push_str(&entities_out.buf);
        if let Some(o) = objects_out {
            all.push_str(&o.buf);
        }
        all.push_str("  0\r\nEOF\r\n");
        all
    }

    fn write_block_shell(
        &mut self,
        out: &mut Out,
        name: &str,
        record: u64,
        base: Point3,
        flags: i64,
        xref: Option<&str>,
    ) {
        out.word(0, "BLOCK");
        if !self.r12 {
            let h = self.new_handle();
            out.handle(5, h);
            out.handle(330, record);
            out.word(100, "AcDbEntity");
        }
        out.word(8, "0");
        if !self.r12 {
            out.word(100, "AcDbBlockBegin");
        }
        out.text(2, name);
        out.int(70, flags);
        out.point(10, base);
        out.text(3, name);
        out.text(1, xref.unwrap_or(""));
    }

    fn end_block(&mut self, out: &mut Out, record: u64) {
        out.word(0, "ENDBLK");
        if !self.r12 {
            let h = self.new_handle();
            out.handle(5, h);
            out.handle(330, record);
            out.word(100, "AcDbEntity");
        }
        out.word(8, "0");
        if !self.r12 {
            out.word(100, "AcDbBlockEnd");
        }
    }

    fn write_header(&mut self, out: &mut Out) {
        let doc = self.doc;
        out.word(0, "SECTION");
        out.word(2, "HEADER");
        out.word(9, "$ACADVER");
        out.word(1, self.ver.acadver());
        if !self.r12 {
            out.word(9, "$DWGCODEPAGE");
            out.word(3, "ANSI_1252");
        }
        out.word(9, "$INSBASE");
        out.point(10, Point3::default());
        let model_idx = doc
            .models
            .iter()
            .position(|m| m.kind == ModelKind::Model)
            .unwrap_or(0);
        let bb = doc.bbox(model_idx);
        out.word(9, "$EXTMIN");
        out.point(10, bb.map_or(Point3::new(1e20, 1e20, 1e20), |b| b.min));
        out.word(9, "$EXTMAX");
        out.point(10, bb.map_or(Point3::new(-1e20, -1e20, -1e20), |b| b.max));
        out.word(9, "$LIMMIN");
        out.point2(10, 0.0, 0.0);
        out.word(9, "$LIMMAX");
        out.point2(10, 420.0, 297.0);
        out.word(9, "$LTSCALE");
        out.real(40, 1.0);
        out.word(9, "$TEXTSTYLE");
        out.word(7, "Standard");
        out.word(9, "$CLAYER");
        out.word(8, "0");
        out.word(9, "$CELTYPE");
        out.word(6, "ByLayer");
        out.word(9, "$CECOLOR");
        out.int(62, 256);
        out.word(9, "$LUNITS");
        out.int(70, 2);
        out.word(9, "$LUPREC");
        out.int(70, 4);
        if !self.r12 {
            out.word(9, "$INSUNITS");
            let code = if doc.units.unit == cadkit_core::LengthUnit::Custom {
                match doc.units.meters_per_unit {
                    Some(m) if (m - 100.0 / 3937.0).abs() < 1e-9 => 22,
                    Some(m) if (m - 3600.0 / 3937.0).abs() < 1e-9 => 23,
                    Some(m) if (m - 6_336_000.0 / 3937.0).abs() < 1e-6 => 24,
                    _ => 0,
                }
            } else {
                i64::from(doc.units.unit.to_insunits())
            };
            out.int(70, code);
            out.word(9, "$TILEMODE");
            out.int(70, 1);
            out.word(9, "$DIMSTYLE");
            out.word(2, "Standard");
            out.word(9, "$HANDSEED");
            out.handle(5, self.next_handle);
        }
        out.word(0, "ENDSEC");
    }

    fn write_classes(&mut self) -> Out {
        let mut out = self.out();
        out.word(0, "SECTION");
        out.word(2, "CLASSES");
        // Group 91 (instance count) exists from R2004 on.
        let ge2004 = self.ge2004;
        let class =
            |out: &mut Out, name: &str, cpp: &str, app: &str, flags: i64, is_entity: bool| {
                out.word(0, "CLASS");
                out.word(1, name);
                out.word(2, cpp);
                out.word(3, app);
                out.int(90, flags);
                if ge2004 {
                    out.int(91, 0);
                }
                out.int(280, 0);
                out.int(281, i64::from(is_entity));
            };
        class(
            &mut out,
            "ACDBDICTIONARYWDFLT",
            "AcDbDictionaryWithDefault",
            "ObjectDBX Classes",
            0,
            false,
        );
        class(
            &mut out,
            "ACDBPLACEHOLDER",
            "AcDbPlaceHolder",
            "ObjectDBX Classes",
            0,
            false,
        );
        class(
            &mut out,
            "LAYOUT",
            "AcDbLayout",
            "ObjectDBX Classes",
            0,
            false,
        );
        if !self.images.is_empty() {
            class(&mut out, "IMAGE", "AcDbRasterImage", "ISM", 127, true);
            class(&mut out, "IMAGEDEF", "AcDbRasterImageDef", "ISM", 0, false);
            class(
                &mut out,
                "IMAGEDEF_REACTOR",
                "AcDbRasterImageDefReactor",
                "ISM",
                1,
                false,
            );
        }
        out.word(0, "ENDSEC");
        out
    }

    // ---- tables -------------------------------------------------------------------------

    fn table_head(&mut self, out: &mut Out, name: &str, count: usize) -> u64 {
        out.word(0, "TABLE");
        out.word(2, name);
        let mut h = 0;
        if !self.r12 {
            h = self.new_handle();
            out.handle(5, h);
            out.handle(330, 0);
            out.word(100, "AcDbSymbolTable");
        }
        out.int(70, count as i64);
        if !self.r12 && name == "DIMSTYLE" {
            out.word(100, "AcDbDimStyleTable");
            out.int(71, count as i64);
        }
        h
    }

    fn record_head(&mut self, out: &mut Out, etype: &str, table: u64, sub: &str, name: &str) {
        out.word(0, etype);
        if !self.r12 {
            let h = self.new_handle();
            out.handle(if etype == "DIMSTYLE" { 105 } else { 5 }, h);
            out.handle(330, table);
            out.word(100, "AcDbSymbolTableRecord");
            out.word(100, sub);
        }
        out.text(2, name);
    }

    fn write_tables(
        &mut self,
        model: &Space,
        layouts: &[Space],
        doc_blocks: &[(String, u64, &cadkit_core::Block)],
    ) -> Out {
        let doc = self.doc;
        let r12 = self.r12;
        let mut out = self.out();
        out.word(0, "SECTION");
        out.word(2, "TABLES");

        // VPORT
        if !r12 {
            let t = self.table_head(&mut out, "VPORT", 1);
            self.record_head(&mut out, "VPORT", t, "AcDbViewportTableRecord", "*Active");
            let model_idx = doc
                .models
                .iter()
                .position(|m| m.kind == ModelKind::Model)
                .unwrap_or(0);
            let (cx, cy, h) = match doc.bbox(model_idx) {
                Some(b) => {
                    let w = (b.max.x - b.min.x).abs();
                    let hh = (b.max.y - b.min.y).abs();
                    (
                        (b.min.x + b.max.x) / 2.0,
                        (b.min.y + b.max.y) / 2.0,
                        (hh.max(w / 1.34) * 1.1).max(1.0),
                    )
                }
                None => (0.0, 0.0, 1000.0),
            };
            out.int(70, 0);
            out.point2(10, 0.0, 0.0);
            out.point2(11, 1.0, 1.0);
            out.point2(12, cx, cy);
            out.point2(13, 0.0, 0.0);
            out.point2(14, 0.5, 0.5);
            out.point2(15, 0.5, 0.5);
            out.point(16, Point3::new(0.0, 0.0, 1.0));
            out.point(17, Point3::default());
            out.real(40, h);
            out.real(41, 1.34);
            out.real(42, 50.0);
            out.real(43, 0.0);
            out.real(44, 0.0);
            out.real(50, 0.0);
            out.real(51, 0.0);
            out.int(71, 0);
            out.int(72, 1000);
            out.int(73, 1);
            out.int(74, 3);
            out.int(75, 0);
            out.int(76, 0);
            out.int(77, 0);
            out.int(78, 0);
            out.word(0, "ENDTAB");
        }

        // LTYPE
        {
            let continuous = self.continuous.clone();
            let mut list: Vec<(String, Option<String>, Vec<f64>)> = Vec::new();
            let mut have: std::collections::HashSet<String> = Default::default();
            for lt in &doc.linetypes {
                let lower = lt.name.trim().to_lowercase();
                if matches!(lower.as_str(), "bylayer" | "byblock" | "continuous" | "") {
                    continue;
                }
                let name = self.linetypes.add(&lt.name);
                if !have.insert(name.to_lowercase()) {
                    continue;
                }
                list.push((name, lt.description.clone(), lt.pattern.clone()));
            }
            // Referenced but undefined linetypes become plain placeholders.
            let mut referenced: Vec<String> = Vec::new();
            let mut note = |n: &str| {
                let lower = n.trim().to_lowercase();
                if !matches!(lower.as_str(), "bylayer" | "byblock" | "continuous" | "") {
                    referenced.push(n.to_owned());
                }
            };
            for l in &doc.layers {
                if let Some(lt) = &l.linetype {
                    note(lt);
                }
            }
            let all = doc
                .models
                .iter()
                .map(|m| m.entities.as_slice())
                .chain(doc.blocks.iter().map(|b| b.entities.as_slice()));
            for l in all {
                walk(
                    l,
                    &mut |e| {
                        if let Some(lt) = &e.linetype {
                            note(lt);
                        }
                    },
                    0,
                );
            }
            for n in referenced {
                let name = self.linetypes.add(&n);
                if have.insert(name.to_lowercase()) {
                    list.push((name, None, Vec::new()));
                }
            }
            let count = list.len() + if r12 { 1 } else { 3 };
            let t = self.table_head(&mut out, "LTYPE", count);
            let simple = |this: &mut Self, out: &mut Out, name: &str, desc: &str| {
                this.record_head(out, "LTYPE", t, "AcDbLinetypeTableRecord", name);
                out.int(70, 0);
                out.text(3, desc);
                out.int(72, 65);
                out.int(73, 0);
                out.real(40, 0.0);
            };
            if !r12 {
                simple(self, &mut out, "ByBlock", "");
                simple(self, &mut out, "ByLayer", "");
            }
            simple(self, &mut out, &continuous, "Solid line");
            for (name, desc, pattern) in list {
                self.record_head(&mut out, "LTYPE", t, "AcDbLinetypeTableRecord", &name);
                out.int(70, 0);
                out.text(3, desc.as_deref().unwrap_or(""));
                out.int(72, 65);
                out.int(73, pattern.len() as i64);
                out.real(40, pattern.iter().map(|d| d.abs()).sum());
                for d in &pattern {
                    out.real(49, *d);
                    if !r12 {
                        out.int(74, 0);
                    }
                }
            }
            out.word(0, "ENDTAB");
        }

        // LAYER
        {
            let mut list: Vec<&cadkit_core::Layer> = Vec::new();
            let mut names: BTreeSet<String> = BTreeSet::new();
            for l in &doc.layers {
                let n = self.layers.add(&l.name);
                if names.insert(n.to_lowercase()) {
                    list.push(l);
                }
            }
            let mut extra: Vec<String> = Vec::new();
            // Layers only referenced by entities.
            let mut referenced: Vec<String> = vec!["0".to_owned()];
            let all = doc
                .models
                .iter()
                .map(|m| m.entities.as_slice())
                .chain(doc.blocks.iter().map(|b| b.entities.as_slice()));
            for l in all {
                walk(
                    l,
                    &mut |e| referenced.push(e.layer.clone().unwrap_or_else(|| "0".to_owned())),
                    0,
                );
            }
            for r in referenced {
                let n = self.layers.add(&r);
                if names.insert(n.to_lowercase()) {
                    extra.push(n);
                }
            }
            let t = self.table_head(&mut out, "LAYER", list.len() + extra.len());
            let continuous = self.continuous.clone();
            // Layer 0 first when it is only implied.
            let mut order: Vec<(String, Option<&cadkit_core::Layer>)> = Vec::new();
            for n in extra.iter().filter(|n| n.as_str() == "0") {
                order.push((n.clone(), None));
            }
            for l in &list {
                order.push((self.layers.add(&l.name), Some(*l)));
            }
            for n in extra.iter().filter(|n| n.as_str() != "0") {
                order.push((n.clone(), None));
            }
            for (name, layer) in order {
                self.record_head(&mut out, "LAYER", t, "AcDbLayerTableRecord", &name);
                let mut flags = 0;
                let mut color = Color::Aci { index: 7 };
                let mut visible = true;
                let mut lw = Lineweight::Default;
                let mut plot = true;
                let mut lt = continuous.clone();
                if let Some(l) = layer {
                    if l.frozen {
                        flags |= 1;
                    }
                    if l.locked {
                        flags |= 4;
                    }
                    color = l.color;
                    visible = l.visible;
                    lw = l.lineweight;
                    plot = l.plottable;
                    if let Some(x) = l.linetype.as_deref().and_then(|x| self.linetype_name(x)) {
                        lt = if x == "ByBlock" {
                            continuous.clone()
                        } else {
                            x
                        };
                    }
                }
                let (aci, rgb) = entities::color_codes(color);
                let aci = if matches!(color, Color::ByLayer | Color::ByBlock) || aci == 0 {
                    7
                } else {
                    aci
                };
                out.int(70, flags);
                out.int(62, if visible { aci } else { -aci });
                if self.ge2004 {
                    if let Some(v) = rgb {
                        out.int(420, v);
                    }
                }
                out.text(6, &lt);
                if !r12 {
                    if !plot {
                        out.int(290, 0);
                    }
                    out.int(370, entities::lineweight_code(lw, -3));
                    out.handle(390, self.plotstyle);
                }
            }
            out.word(0, "ENDTAB");
        }

        // STYLE
        {
            let mut list: Vec<(String, f64, f64, f64, String)> = Vec::new();
            let mut seen: BTreeSet<String> = BTreeSet::new();
            for s in &doc.text_styles {
                let n = self.styles.add(&s.name);
                if seen.insert(n.to_lowercase()) {
                    let font = s.font.clone().unwrap_or_else(|| "txt".to_owned());
                    let wf = if s.width_factor > 0.0 {
                        s.width_factor
                    } else {
                        1.0
                    };
                    list.push((n, s.height.max(0.0), wf, s.oblique.to_degrees(), font));
                }
            }
            let mut referenced: Vec<String> = vec!["Standard".to_owned()];
            let all = doc
                .models
                .iter()
                .map(|m| m.entities.as_slice())
                .chain(doc.blocks.iter().map(|b| b.entities.as_slice()));
            for l in all {
                walk(
                    l,
                    &mut |e| match &e.kind {
                        EntityKind::Text { style: Some(s), .. }
                        | EntityKind::MText { style: Some(s), .. } => referenced.push(s.clone()),
                        _ => {}
                    },
                    0,
                );
            }
            for r in referenced {
                if r.trim().is_empty() {
                    continue;
                }
                let n = self.styles.add(&r);
                if seen.insert(n.to_lowercase()) {
                    list.push((n, 0.0, 1.0, 0.0, "txt".to_owned()));
                }
            }
            // Standard first.
            list.sort_by_key(|(n, ..)| !n.eq_ignore_ascii_case("Standard"));
            let t = self.table_head(&mut out, "STYLE", list.len());
            for (name, h, wf, obl, font) in list {
                self.record_head(&mut out, "STYLE", t, "AcDbTextStyleTableRecord", &name);
                out.int(70, 0);
                out.real(40, h);
                out.real(41, wf);
                out.real(50, obl);
                out.int(71, 0);
                out.real(42, if h > 0.0 { h } else { 2.5 });
                out.text(3, &font);
                out.text(4, "");
            }
            out.word(0, "ENDTAB");
        }

        // VIEW, UCS
        if !r12 {
            for name in ["VIEW", "UCS"] {
                self.table_head(&mut out, name, 0);
                out.word(0, "ENDTAB");
            }
        }

        // APPID
        {
            let mut apps: BTreeSet<String> = self.appids.clone();
            apps.remove("ACAD");
            let t = self.table_head(&mut out, "APPID", apps.len() + 1);
            self.record_head(&mut out, "APPID", t, "AcDbRegAppTableRecord", "ACAD");
            out.int(70, 0);
            for a in apps {
                self.record_head(&mut out, "APPID", t, "AcDbRegAppTableRecord", &a);
                out.int(70, 0);
            }
            out.word(0, "ENDTAB");
        }

        // DIMSTYLE
        {
            let mut names: Vec<String> = vec!["Standard".to_owned()];
            let mut all: Vec<String> = Vec::new();
            for v in self.dimstyles.map.values() {
                all.push(v.clone());
            }
            all.sort();
            for n in all {
                if !n.eq_ignore_ascii_case("Standard") {
                    names.push(n);
                }
            }
            let std_style = 0u64;
            let t = self.table_head(&mut out, "DIMSTYLE", names.len());
            for n in names {
                self.record_head(&mut out, "DIMSTYLE", t, "AcDbDimStyleTableRecord", &n);
                entities::dimstyle_defaults(&mut out, r12, std_style);
            }
            out.word(0, "ENDTAB");
        }

        // BLOCK_RECORD
        if !r12 {
            let count = 2 + layouts.len().saturating_sub(1) + doc_blocks.len();
            let t = self.table_head(&mut out, "BLOCK_RECORD", count);
            let rec = |this: &mut Self, out: &mut Out, name: &str, handle: u64, layout: u64| {
                out.word(0, "BLOCK_RECORD");
                out.handle(5, handle);
                out.handle(330, t);
                out.word(100, "AcDbSymbolTableRecord");
                out.word(100, "AcDbBlockTableRecord");
                out.text(2, name);
                if layout != 0 {
                    out.handle(340, layout);
                }
                if this.ge2007 {
                    out.int(70, 0);
                    out.int(280, 1);
                    out.int(281, 0);
                }
            };
            rec(
                self,
                &mut out,
                "*Model_Space",
                model.block_record,
                model.layout,
            );
            for l in layouts {
                rec(self, &mut out, &l.block_name, l.block_record, l.layout);
            }
            for (name, h, _) in doc_blocks {
                rec(self, &mut out, name, *h, 0);
            }
            out.word(0, "ENDTAB");
        }

        out.word(0, "ENDSEC");
        out
    }

    // ---- objects ------------------------------------------------------------------------

    fn dict_head(&mut self, out: &mut Out, handle: u64, owner: u64, entries: &[(String, u64)]) {
        out.word(0, "DICTIONARY");
        out.handle(5, handle);
        out.handle(330, owner);
        out.word(100, "AcDbDictionary");
        out.int(281, 1);
        for (name, h) in entries {
            out.text(3, name);
            out.handle(350, *h);
        }
    }

    fn write_objects(
        &mut self,
        model: &Space,
        layouts: &[Space],
        root: u64,
        group: u64,
        layout_dict: u64,
    ) -> Out {
        let mut out = self.out();
        out.word(0, "SECTION");
        out.word(2, "OBJECTS");

        let plot_dict = self.new_handle();
        let plotstyle_dict = self.new_handle();
        let image_dict = if self.images.is_empty() {
            0
        } else {
            self.new_handle()
        };
        let mut root_entries = vec![
            ("ACAD_GROUP".to_owned(), group),
            ("ACAD_LAYOUT".to_owned(), layout_dict),
            ("ACAD_PLOTSETTINGS".to_owned(), plot_dict),
            ("ACAD_PLOTSTYLENAME".to_owned(), plotstyle_dict),
        ];
        if image_dict != 0 {
            root_entries.push(("ACAD_IMAGE_DICT".to_owned(), image_dict));
        }
        self.dict_head(&mut out, root, 0, &root_entries);
        self.dict_head(&mut out, group, root, &[]);
        self.dict_head(&mut out, plot_dict, root, &[]);
        // Plot style table: a dictionary with a default entry pointing at a placeholder.
        out.word(0, "ACDBDICTIONARYWDFLT");
        out.handle(5, plotstyle_dict);
        out.handle(330, root);
        out.word(100, "AcDbDictionary");
        out.int(281, 1);
        out.word(3, "Normal");
        out.handle(350, self.plotstyle);
        out.word(100, "AcDbDictionaryWithDefault");
        out.handle(340, self.plotstyle);
        out.word(0, "ACDBPLACEHOLDER");
        out.handle(5, self.plotstyle);
        out.handle(330, plotstyle_dict);
        let mut layout_entries = vec![("Model".to_owned(), model.layout)];
        for l in layouts {
            layout_entries.push((l.name.clone(), l.layout));
        }
        self.dict_head(&mut out, layout_dict, root, &layout_entries);

        // Images: dictionary, IMAGEDEF per distinct path, one reactor per IMAGE.
        if image_dict != 0 {
            let images = std::mem::take(&mut self.images);
            let mut defs: Vec<(String, u64, Vec<u64>, [u32; 2])> = Vec::new();
            let mut by_path: HashMap<&str, usize> = HashMap::new();
            for img in &images {
                match by_path
                    .get(img.path.as_str())
                    .and_then(|&i| defs.get_mut(i))
                {
                    Some((_, _, reactors, _)) => reactors.push(img.reactor),
                    None => {
                        by_path.insert(img.path.as_str(), defs.len());
                        defs.push((img.path.clone(), img.def, vec![img.reactor], img.size));
                    }
                }
            }
            let mut used = UniqueNames::default();
            let mut entries = Vec::new();
            for (path, def, _, _) in &defs {
                let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
                let stem = base.rsplit_once('.').map_or(base, |(s, _)| s);
                let name = used.make(&clean_name(stem));
                entries.push((name, *def));
            }
            self.dict_head(&mut out, image_dict, root, &entries);
            for (path, def, reactors, size) in &defs {
                out.word(0, "IMAGEDEF");
                out.handle(5, *def);
                out.word(102, "{ACAD_REACTORS");
                out.handle(330, image_dict);
                for r in reactors {
                    out.handle(330, *r);
                }
                out.word(102, "}");
                out.handle(330, image_dict);
                out.word(100, "AcDbRasterImageDef");
                out.int(90, 0);
                out.text(1, path);
                out.point2(10, f64::from(size[0]), f64::from(size[1]));
                out.point2(11, 1.0, 1.0);
                out.int(280, 1);
                out.int(281, 0);
            }
            for img in &images {
                out.word(0, "IMAGEDEF_REACTOR");
                out.handle(5, img.reactor);
                out.handle(330, img.entity);
                out.word(100, "AcDbRasterImageDefReactor");
                out.int(90, 2);
                out.handle(330, img.entity);
            }
            self.images = images;
        }

        let mut all_layouts: Vec<(&str, u64, u64, i64)> =
            vec![("Model", model.layout, model.block_record, 0)];
        for (i, l) in layouts.iter().enumerate() {
            all_layouts.push((l.name.as_str(), l.layout, l.block_record, i as i64 + 1));
        }
        for (name, handle, record, tab) in all_layouts {
            let is_model = tab == 0;
            out.word(0, "LAYOUT");
            out.handle(5, handle);
            out.handle(330, layout_dict);
            out.word(100, "AcDbPlotSettings");
            out.text(1, "");
            out.text(4, "");
            out.text(6, "");
            out.real(40, 0.0);
            out.real(41, 0.0);
            out.real(42, 0.0);
            out.real(43, 0.0);
            out.real(44, 0.0);
            out.real(45, 0.0);
            out.real(46, 0.0);
            out.real(47, 0.0);
            out.real(48, 0.0);
            out.real(49, 0.0);
            out.real(140, 0.0);
            out.real(141, 0.0);
            out.real(142, 1.0);
            out.real(143, 1.0);
            out.int(70, if is_model { 1024 } else { 0 });
            out.int(72, 0);
            out.int(73, 0);
            out.int(74, 5);
            out.text(7, "");
            out.int(75, 16);
            out.int(76, 0);
            out.int(77, 2);
            out.int(78, 300);
            out.real(147, 1.0);
            out.real(148, 0.0);
            out.real(149, 0.0);
            out.word(100, "AcDbLayout");
            out.text(1, name);
            out.int(70, 1);
            out.int(71, tab);
            out.point2(10, 0.0, 0.0);
            out.point2(11, 420.0, 297.0);
            out.real(12, 0.0);
            out.real(22, 0.0);
            out.real(32, 0.0);
            out.point(14, Point3::new(1e20, 1e20, 1e20));
            out.point(15, Point3::new(-1e20, -1e20, -1e20));
            out.real(146, 0.0);
            out.point(13, Point3::default());
            out.point(16, Point3::new(1.0, 0.0, 0.0));
            out.point(17, Point3::new(0.0, 1.0, 0.0));
            out.int(76, 0);
            out.handle(330, record);
        }
        out.word(0, "ENDSEC");
        out
    }
}

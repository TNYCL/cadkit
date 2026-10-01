//! Orchestration: container → header, classes, object map → objects.

use cadkit_core::{Error, ReadOptions, Result, Warning};

use crate::native::{ClassTable, DwgFile, HeaderVars};
use crate::object::{DecodeContext, parse_object};
use crate::{classes, codepage, container, handles, header};

/// Per-code cap on individual warnings; the rest are summarized.
const MAX_WARNINGS_PER_CODE: usize = 50;

struct Warnings {
    list: Vec<Warning>,
    suppressed: std::collections::BTreeMap<String, usize>,
}

impl Warnings {
    fn push(&mut self, w: Warning) {
        let same = self.list.iter().filter(|x| x.code == w.code).count();
        if same < MAX_WARNINGS_PER_CODE {
            self.list.push(w);
        } else {
            *self.suppressed.entry(w.code).or_default() += 1;
        }
    }

    fn add(&mut self, code: &str, message: String, offset: Option<u64>, object: Option<u64>) {
        self.push(Warning {
            code: code.to_owned(),
            message,
            offset,
            object,
        });
    }

    fn finish(mut self) -> Vec<Warning> {
        for (code, n) in self.suppressed {
            self.list.push(Warning {
                code: code.clone(),
                message: format!("{n} more {code} warnings suppressed"),
                offset: None,
                object: None,
            });
        }
        self.list
    }
}

/// Reads the native model. `keep_raw` keeps every object's bytes.
pub fn read_file(bytes: &[u8], options: &ReadOptions, keep_raw: bool) -> Result<DwgFile> {
    let limits = &options.limits;
    let mut c = container::read(bytes, limits)?;
    let mut w = Warnings {
        list: Vec::new(),
        suppressed: Default::default(),
    };
    for warning in std::mem::take(&mut c.warnings) {
        w.push(warning);
    }
    let version = c.version;
    let (encoding, from_file) = codepage::resolve(c.codepage, options.fallback_codepage.as_deref());
    if !from_file && !version.r2007_plus() {
        w.add(
            "dwg.codepage_fallback",
            format!(
                "code page {} is not supported; decoding 8-bit text as {}",
                c.codepage,
                encoding.name()
            ),
            Some(0x13),
            None,
        );
    }
    let ctx = DecodeContext {
        version,
        encoding,
        max_string: limits.max_string_bytes,
        max_vertices: limits.max_vertices,
    };

    let header = match &c.header {
        Some(section) => {
            let at = section.warning_offset(section.base);
            let in_section = container::in_section(section);
            match header::parse(section, c.maintenance, ctx) {
                Ok((vars, error)) => {
                    if let Some(e) = error {
                        w.add(
                            "dwg.header_incomplete",
                            format!("header variables: {e}{}", section.offsets_note()),
                            at,
                            None,
                        );
                    }
                    if vars.crc_ok == Some(false) {
                        w.add(
                            "dwg.crc_mismatch",
                            "header variables CRC or end sentinel mismatch".into(),
                            at,
                            None,
                        );
                    }
                    vars
                }
                Err(e) => {
                    w.add(
                        "dwg.header_error",
                        format!("header variables: {}", in_section(e)),
                        at,
                        None,
                    );
                    HeaderVars::default()
                }
            }
        }
        None => {
            w.add(
                "dwg.missing_section",
                "no header variables section".into(),
                None,
                None,
            );
            HeaderVars::default()
        }
    };
    let measurement = c.template.as_ref().and_then(header::parse_template);

    let mut classes_crc_ok = None;
    let classes = match &c.classes {
        Some(section) => {
            let at = section.warning_offset(section.base);
            let in_section = container::in_section(section);
            match classes::parse(section, c.maintenance, ctx, limits.max_objects) {
                Ok((list, error, trailer_ok)) => {
                    classes_crc_ok = trailer_ok;
                    if let Some(e) = error {
                        w.add(
                            "dwg.classes_incomplete",
                            format!("classes: {e}{}", section.offsets_note()),
                            at,
                            None,
                        );
                    }
                    if trailer_ok == Some(false) {
                        w.add(
                            "dwg.crc_mismatch",
                            "classes section CRC or end sentinel mismatch".into(),
                            at,
                            None,
                        );
                    }
                    list
                }
                Err(e) => {
                    w.add(
                        "dwg.classes_error",
                        format!("classes: {}", in_section(e)),
                        at,
                        None,
                    );
                    Vec::new()
                }
            }
        }
        None => Vec::new(),
    };
    let class_table = ClassTable::new(&classes);

    let map_section = c
        .handles
        .as_ref()
        .ok_or_else(|| Error::invalid(0, "no object map section"))?;
    let objects_section = c
        .objects
        .as_ref()
        .ok_or_else(|| Error::invalid(0, "no object data section"))?;
    let stream: &[u8] = &objects_section.data;
    // An object takes at least 4 bytes (size, type and handle bits, CRC), so a map
    // with more entries than that is corrupt; the excess is ignored.
    let max_entries = stream.len() / 4 + 1;
    let map = handles::parse(
        &map_section.data,
        map_section.base,
        limits.max_objects,
        max_entries,
    )
    .map_err(container::in_section(map_section))?;
    let map_at = map_section.warning_offset(map_section.base);
    if map.crc_failures > 0 {
        w.add(
            "dwg.crc_mismatch",
            format!("{} object map chunks with CRC mismatch", map.crc_failures),
            map_at,
            None,
        );
    }
    if let Some(at) = map.truncated_at {
        w.add(
            "dwg.object_map_truncated",
            format!(
                "object map ends early at {}; {} entries read",
                map_section.locate(at),
                map.map.len()
            ),
            map_section.warning_offset(at),
            None,
        );
    }
    if map.dropped > 0 {
        w.add(
            "dwg.object_map",
            format!("{} invalid object map entries dropped", map.dropped),
            map_at,
            None,
        );
    }
    if map.duplicates > 0 {
        w.add(
            "dwg.object_map",
            format!(
                "{} object map entries repeat a handle or an offset; dropped",
                map.duplicates
            ),
            map_at,
            None,
        );
    }
    if map.excess > 0 {
        w.add(
            "dwg.object_map",
            format!(
                "object map lists {} more entries than {} bytes of object data can hold; ignored",
                map.excess,
                stream.len()
            ),
            map_at,
            None,
        );
    }

    let mut objects = std::collections::BTreeMap::new();
    let mut crc_failures = 0usize;
    // Offsets inside a decompressed AcDb:AcDbObjects are relative to that section;
    // messages name it and only file offsets go into `Warning::offset`.
    let in_objects = container::in_section(objects_section);
    for (&handle, &offset) in &map.map {
        let at = objects_section.warning_offset(offset);
        let place = objects_section.locate(offset);
        match parse_object(
            stream,
            objects_section.base,
            offset,
            ctx,
            &class_table,
            keep_raw,
        ) {
            Ok(obj) => {
                if obj.handle != handle {
                    w.add(
                        "dwg.handle_mismatch",
                        format!(
                            "object at {place} has handle {:X}, map says {handle:X}",
                            obj.handle
                        ),
                        at,
                        Some(handle),
                    );
                }
                if obj.crc_ok == Some(false) {
                    crc_failures += 1;
                }
                if let Some(e) = &obj.error {
                    w.add(
                        "dwg.decode_error",
                        format!("{} {handle:X} at {place}: {e}", obj.type_name),
                        at,
                        Some(handle),
                    );
                }
                for warning in &obj.warnings {
                    let message = format!(
                        "{} {handle:X} at {place}: {}",
                        obj.type_name, warning.message
                    );
                    w.add(&warning.code, message, at, Some(handle));
                }
                objects.insert(handle, obj);
            }
            Err(e) => w.add(
                "dwg.object_error",
                format!("object {handle:X} at {place}: {}", in_objects(e)),
                at,
                Some(handle),
            ),
        }
    }
    if crc_failures > 0 {
        w.add(
            "dwg.crc_mismatch",
            format!("{crc_failures} objects with CRC mismatch"),
            None,
            None,
        );
    }

    Ok(DwgFile {
        version,
        maintenance_version: c.maintenance,
        codepage: c.codepage,
        encoding: encoding.name(),
        header,
        measurement,
        class_index: class_table.into_index(),
        classes,
        classes_crc_ok,
        handle_map: map.map,
        objects,
        section_names: c.section_names,
        warnings: w.finish(),
    })
}

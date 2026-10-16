//! New raster attachments cloned from a raster attachment of the seed, or from the built-in
//! group of [`super::raster_builtin`] when the seed holds none.
//!
//! A MicroStation raster attachment is a fixed group of ten records and one XAttribute
//! (FORMAT_NOTES, "Raster attachments"): control records 92, 93, 93, 90 and five 91 in
//! `Dgn^C`, the frame 94 in the graphic pages and an XAttribute of kind 170 on the frame in
//! `Dgn^GA`. Many fields of those records are not decoded, so a new attachment is a copy of
//! a complete group of the seed in which only documented fields are rewritten: record ids
//! and times, the 90 <-> 92 cross ids, the two dependency roots on the frame, the path
//! strings, the placement, the resolution and the level.

use super::encode::*;
use crate::le;
use crate::native::linkage::{self, LinkageData};
use crate::native::v8::{AuxRecord, ModelHeader, RawElement, V8Model};
use cadkit_core::{Entity, EntityKind, Error, Point3, Result, Value, Vec3};
use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_2, TAU};

/// Dependency application id of the raster attachment roots (90 and 92 -> 94).
const DEP_RASTER_FRAME: u16 = 0x271b;
/// XAttribute kind holding the raster attachment strings.
const XA_RASTER: u32 = 170;
/// Second `u16` of every XAttribute 170 sub-record header.
const XA_SUB_MAGIC: u16 = 0x570b;
/// Sub-records of an XAttribute 170; 3 holds the file name, 4 the `file://` path.
const XA_SUBS: usize = 8;
/// Subtype (`u32` at `0x20`) of the 91 record holding the placement.
const SETTINGS_PLACEMENT: u32 = 0x2_0007;
/// Smallest frame record that holds every rewritten field (DPI ends at `0x128`).
const FRAME_MIN: usize = 0x128;
/// Smallest placement record (its second axis ends at `0xB8`).
const PLACEMENT_MIN: usize = 0xb8;
/// Records of one group in `Dgn^C`, in stored order: 92, 93, 93, 90 and five 91.
const CONTROLS: usize = 9;

/// Errors with stable messages.
const NEEDS_PATH_AND_SIZE: &str = "new DGN raster attachments need path and size_px";

/// One complete raster attachment group of the seed, captured before the seed model is
/// cleared.
#[derive(Debug, Clone)]
pub(super) struct Template {
    frame: Vec<u8>,
    link: Vec<u8>,
    /// Offset of the frame id in the 92 dependency linkage.
    link_root: usize,
    options: [Vec<u8>; 2],
    /// The 90 record up to its linkages.
    reference: Vec<u8>,
    /// The 90 dependency linkage on the frame.
    reference_dependency: Vec<u8>,
    settings: Vec<Vec<u8>>,
    /// Which of `settings` holds the placement (subtype `0x20007`).
    placement: usize,
    xattribute: XAttribute,
    /// Format version of the auxiliary page holding the template XAttribute.
    pub aux_version: u32,
}

/// XAttribute 170 framing: eight sub-records `{u16 index, u16 0x570B, u32, u16 length,
/// u16 flags, u32, data[length]}` and a short trailer.
#[derive(Debug, Clone)]
struct XAttribute {
    reserved: u32,
    flags: u32,
    subs: Vec<([u8; 16], Vec<u8>)>,
    trailer: Vec<u8>,
}

/// The records of one new attachment.
pub(super) struct Group {
    pub frame_id: u64,
    pub frame: Vec<u8>,
    /// Control records in stored order (92, 93, 93, 90, 91 x 5).
    pub controls: Vec<Vec<u8>>,
    pub xattribute: AuxRecord,
}

impl Template {
    /// The first complete raster attachment group of `model`, if any.
    pub fn capture(model: &V8Model) -> Option<Self> {
        let controls: Vec<&RawElement> = model
            .control_pages
            .iter()
            .filter(|p| p.complete)
            .flat_map(|p| &p.elements)
            .collect();
        let frames: HashMap<u64, &RawElement> = model
            .graphic_pages
            .iter()
            .filter(|p| p.complete)
            .flat_map(|p| &p.elements)
            .filter(|r| r.type_code() == 94)
            .filter_map(|r| Some((r.id()?, r)))
            .collect();
        let mut xattributes: HashMap<u64, Vec<(&AuxRecord, u32)>> = HashMap::new();
        for page in model.graphic_aux.iter().filter(|p| p.complete) {
            for a in page.records.iter().filter(|a| a.kind == XA_RASTER) {
                xattributes
                    .entry(a.element_id)
                    .or_default()
                    .push((a, page.header.format_version));
            }
        }
        (0..controls.len()).find_map(|i| group(&controls, i, &frames, &xattributes))
    }

    /// The built-in group of [`super::raster_builtin`], for seeds that hold no attachment;
    /// its XAttribute goes in auxiliary pages of format `aux_version`.
    pub fn builtin(aux_version: u32) -> Result<Self> {
        use super::raster_builtin as b;
        let mut subs = Vec::with_capacity(XA_SUBS);
        for (index, data) in (0u16..).zip(&b::XATTRIBUTE_SUBS) {
            let data = data.bytes()?;
            let mut head = [0u8; 16];
            put(&mut head, 0, &index.to_le_bytes())?;
            put(&mut head, 2, &XA_SUB_MAGIC.to_le_bytes())?;
            let len = u16::try_from(data.len())
                .map_err(|_| Error::LimitExceeded("DGN raster XAttribute string".into()))?;
            put(&mut head, 8, &len.to_le_bytes())?;
            put(&mut head, 10, &b::XATTRIBUTE_SUB_FLAGS.to_le_bytes())?;
            subs.push((head, data));
        }
        let [option_a, option_b] = &b::OPTIONS;
        Ok(Self {
            frame: b::FRAME.bytes()?,
            link: b::LINK.bytes()?,
            link_root: b::LINK_ROOT,
            options: [option_a.bytes()?, option_b.bytes()?],
            reference: b::REFERENCE.bytes()?,
            reference_dependency: b::REFERENCE_DEPENDENCY.bytes()?,
            settings: b::SETTINGS
                .iter()
                .map(b::Sparse::bytes)
                .collect::<Result<_>>()?,
            placement: 0,
            xattribute: XAttribute {
                reserved: 0,
                flags: b::XATTRIBUTE_FLAGS,
                subs,
                trailer: b::XATTRIBUTE_TRAILER.to_vec(),
            },
            aux_version,
        })
    }

    /// Records of a new attachment for `e`: ids are `ids[0..10]` in the order 94, 92, 93,
    /// 93, 90, 91 x 5 (as MicroStation allocates them), the frame sits on `level`.
    pub fn instantiate(
        &self,
        e: &Entity,
        ids: [u64; 10],
        level: u32,
        model: &ModelHeader,
        time: f64,
    ) -> Result<Group> {
        let EntityKind::Image {
            path,
            position,
            u_vector,
            v_vector,
            size_px,
        } = &e.kind
        else {
            return Err(Error::invalid(0, "raster template needs an image entity"));
        };
        let (Some(path), Some(size)) = (path, size_px) else {
            return Err(Error::Unsupported(NEEDS_PATH_AND_SIZE.into()));
        };
        let url = file_url(path)?;
        let name = file_name(e, path)?;
        let p = Placement::new(*position, *u_vector, *v_vector, *size, model)?;
        let dpi = self.dpi(e)?;
        let [frame_id, link_id, option_a, option_b, reference_id, ..] = ids;

        let mut frame = self.frame.clone();
        identity(&mut frame, frame_id, time)?;
        u32_at(&mut frame, 0x0c, level)?;
        let props = le::u32_at(&frame, 0x28).unwrap_or(0) & !0x80;
        u32_at(&mut frame, 0x28, props | if e.visible { 0 } else { 0x80 })?;
        range(&mut frame, &p.corners())?;
        let [ax, ay, az] = [p.a.x, p.a.y, p.a.z];
        let [bx, by, bz] = [p.b.x, p.b.y, p.b.z];
        let [nx, ny, nz] = [p.n.x, p.n.y, p.n.z];
        let [tx, ty, tz] = p.origin;
        let matrix = [
            ax, bx, nx, tx, ay, by, ny, ty, az, bz, nz, tz, 0.0, 0.0, 0.0, 1.0,
        ];
        for (i, v) in matrix.into_iter().enumerate() {
            f64_at(&mut frame, 0x78 + 8 * i, v)?;
        }
        f64_at(&mut frame, 0x108, p.edges[0])?;
        f64_at(&mut frame, 0x110, p.edges[1])?;
        f64_at(&mut frame, 0x118, dpi[0])?;
        f64_at(&mut frame, 0x120, dpi[1])?;

        let mut link = self.link.clone();
        identity(&mut link, link_id, time)?;
        u64_at(&mut link, 0x30, reference_id)?;
        u64_at(&mut link, self.link_root, frame_id)?;

        let mut options = self.options.clone();
        for (b, id) in options.iter_mut().zip([option_a, option_b]) {
            identity(b, id, time)?;
        }

        let mut reference = self.reference.clone();
        identity(&mut reference, reference_id, time)?;
        u64_at(&mut reference, 0x38, link_id)?;
        let mut links = string_link_bytes(3, &linkage_text(&name))?;
        links.extend(string_link_bytes(0x1f, &linkage_text(&url))?);
        let mut dependency = self.reference_dependency.clone();
        u64_at(&mut dependency, 12, frame_id)?;
        links.extend(dependency);
        let reference = finish(reference, &links)?;

        let [opt_a, opt_b] = options;
        let mut controls = vec![link, opt_a, opt_b, reference];
        for (i, (template, id)) in self
            .settings
            .iter()
            .zip(ids.into_iter().skip(5))
            .enumerate()
        {
            let mut b = template.clone();
            identity(&mut b, id, time)?;
            if i == self.placement {
                p.write_settings(&mut b, dpi)?;
            }
            controls.push(b);
        }

        let xattribute = AuxRecord {
            offset: 0,
            kind: XA_RASTER,
            reserved: self.xattribute.reserved,
            element_id: frame_id,
            flags: self.xattribute.flags,
            payload: self.xattribute.payload(&name, &url)?,
        };
        Ok(Group {
            frame_id,
            frame,
            controls,
            xattribute,
        })
    }

    /// Horizontal and vertical resolution: `dgn.raster_dpi` / `dgn.raster_dpi_y`, else the
    /// template's.
    fn dpi(&self, e: &Entity) -> Result<[f64; 2]> {
        let template = [
            le::f64_at(&self.frame, 0x118).unwrap_or(0.0),
            le::f64_at(&self.frame, 0x120).unwrap_or(0.0),
        ];
        let x = number(e, "dgn.raster_dpi")?;
        let y = number(e, "dgn.raster_dpi_y")?;
        Ok([x.unwrap_or(template[0]), y.or(x).unwrap_or(template[1])])
    }
}

/// A positive number prop, or `None` when absent.
fn number(e: &Entity, key: &str) -> Result<Option<f64>> {
    let v = match e.props.get(key) {
        None => return Ok(None),
        Some(Value::Float(v)) => *v,
        Some(Value::Int(v)) => *v as f64,
        Some(_) => return Err(Error::invalid(0, format!("{key} must be a number"))),
    };
    if !(v.is_finite() && v > 0.0) {
        return Err(Error::invalid(0, format!("{key} must be positive")));
    }
    Ok(Some(v))
}

/// The group whose 92 record is `controls[i]`, when every record and link is in place.
fn group(
    controls: &[&RawElement],
    i: usize,
    frames: &HashMap<u64, &RawElement>,
    xattributes: &HashMap<u64, Vec<(&AuxRecord, u32)>>,
) -> Option<Template> {
    let records = controls.get(i..i.checked_add(CONTROLS)?)?;
    let [link, option_a, option_b, reference, settings @ ..] = records else {
        return None;
    };
    let types: Vec<u16> = records.iter().map(|r| r.type_code()).collect();
    if types.as_slice() != [92, 93, 93, 90, 91, 91, 91, 91, 91]
        || records
            .iter()
            .any(|r| r.prefix != 0 || r.bytes.len() < 0x40)
    {
        return None;
    }
    let (link_id, reference_id) = (link.id()?, reference.id()?);
    if le::u64_at(&reference.bytes, 0x38)? != link_id
        || le::u64_at(&link.bytes, 0x30)? != reference_id
    {
        return None;
    }
    let enc = encoding_rs::WINDOWS_1252;
    let link_links = linkage::parse(link.attributes(), link.attribute_offset(), true, enc);
    let [only] = link_links.as_slice() else {
        return None;
    };
    let frame_id = single_root(only)?;
    let link_root = only.offset.checked_add(12)?;
    let reference_links = linkage::parse(
        reference.attributes(),
        reference.attribute_offset(),
        true,
        enc,
    );
    let [name, path, dependency] = reference_links.as_slice() else {
        return None;
    };
    let is_string = |l: &linkage::Linkage, id: u32| matches!(l.data, LinkageData::String { string_id, .. } if string_id == id);
    if !is_string(name, 3) || !is_string(path, 0x1f) || single_root(dependency)? != frame_id {
        return None;
    }
    let placement = settings
        .iter()
        .position(|r| le::u32_at(&r.bytes, 0x20) == Some(SETTINGS_PLACEMENT))?;
    if settings
        .iter()
        .filter(|r| le::u32_at(&r.bytes, 0x20) == Some(SETTINGS_PLACEMENT))
        .count()
        != 1
        || settings.get(placement)?.bytes.len() < PLACEMENT_MIN
    {
        return None;
    }
    let frame = frames.get(&frame_id)?;
    // A stand-alone frame without linkages, as every observed attachment stores it.
    if frame.bytes.len() < FRAME_MIN
        || frame.type_word & 0x6000_0000 != 0
        || frame.prefix != 0
        || !frame.attributes().iter().all(|b| *b == 0)
    {
        return None;
    }
    let [(xattribute, aux_version)] = xattributes.get(&frame_id)?.as_slice() else {
        return None;
    };
    Some(Template {
        frame: frame.bytes.clone(),
        link: link.bytes.clone(),
        link_root,
        options: [option_a.bytes.clone(), option_b.bytes.clone()],
        reference: reference.primary().to_vec(),
        reference_dependency: dependency.bytes.clone(),
        settings: settings.iter().map(|r| r.bytes.clone()).collect(),
        placement,
        xattribute: XAttribute::parse(xattribute)?,
        aux_version: *aux_version,
    })
}

/// The only root of a raster dependency linkage (application `0x271B`, plain 64-bit ids).
fn single_root(l: &linkage::Linkage) -> Option<u64> {
    match &l.data {
        LinkageData::Dependency {
            app_id: DEP_RASTER_FRAME,
            root_type: 2 | 3,
            roots,
            ..
        } if roots.len() == 1 && le::u16_at(&l.bytes, 10) == Some(1) => roots.first().copied(),
        _ => None,
    }
}

impl XAttribute {
    fn parse(a: &AuxRecord) -> Option<Self> {
        let payload = &a.payload;
        let mut subs = Vec::with_capacity(XA_SUBS);
        let mut off = 0usize;
        while subs.len() < XA_SUBS {
            let head = le::array::<16>(payload, off)?;
            if usize::from(le::u16_at(&head, 0)?) != subs.len()
                || le::u16_at(&head, 2)? != XA_SUB_MAGIC
            {
                return None;
            }
            let len = usize::from(le::u16_at(&head, 8)?);
            let start = off.checked_add(16)?;
            subs.push((head, le::bytes(payload, start, len)?.to_vec()));
            off = start.checked_add(len)?;
        }
        let trailer = payload.get(off..)?.to_vec();
        // Sub-records 3 and 4 must hold exactly one string, which is what gets replaced.
        let single = |i: usize| subs.get(i).and_then(|(_, d)| utf16z_text(d));
        if trailer.len() > 16 || single(3).is_none() || single(4).is_none() {
            return None;
        }
        Some(Self {
            reserved: a.reserved,
            flags: a.flags,
            subs,
            trailer,
        })
    }

    /// The template payload with the file name in sub-record 3 and the URL in 4.
    fn payload(&self, name: &str, url: &str) -> Result<Vec<u8>> {
        let mut out = Vec::new();
        for (i, (head, data)) in self.subs.iter().enumerate() {
            let mut head = *head;
            let data = match i {
                3 => utf16z(name),
                4 => utf16z(url),
                _ => data.clone(),
            };
            let len = u16::try_from(data.len())
                .map_err(|_| Error::LimitExceeded("DGN raster XAttribute string".into()))?;
            put(&mut head, 8, &len.to_le_bytes())?;
            out.extend_from_slice(&head);
            out.extend(data);
        }
        out.extend_from_slice(&self.trailer);
        Ok(out)
    }
}

/// UTF-16LE with a NUL unit, without a marker (XAttribute 170 strings).
fn utf16z(s: &str) -> Vec<u8> {
    s.encode_utf16()
        .chain([0])
        .flat_map(u16::to_le_bytes)
        .collect()
}

/// The text of `data` when it is exactly one NUL-terminated UTF-16LE string.
fn utf16z_text(data: &[u8]) -> Option<String> {
    let units: Vec<u16> = data
        .chunks_exact(2)
        .map(|c| {
            u16::from_le_bytes([
                c.first().copied().unwrap_or(0),
                c.get(1).copied().unwrap_or(0),
            ])
        })
        .collect();
    let (last, body) = units.split_last()?;
    if data.len() % 2 != 0 || *last != 0 || body.is_empty() || body.contains(&0) {
        return None;
    }
    String::from_utf16(body).ok()
}

/// A string linkage text: 8-bit when ASCII, else `ff fd` + UTF-16LE, without terminator
/// (as the attachment records store both forms).
fn linkage_text(s: &str) -> Vec<u8> {
    if s.is_ascii() {
        s.as_bytes().to_vec()
    } else {
        let mut b = vec![0xff, 0xfd];
        b.extend(s.encode_utf16().flat_map(u16::to_le_bytes));
        b
    }
}

/// Record id at `0x10` and time at `0x18`.
fn identity(b: &mut [u8], id: u64, time: f64) -> Result<()> {
    u64_at(b, 0x10, id)?;
    f64_at(b, 0x18, time)
}

/// The stored URL: the path as given when it is already a `file://` URL, else `file://`
/// followed by the path. The path must be absolute (a drive, UNC or rooted path): the
/// attachments carry no relative paths.
fn file_url(path: &str) -> Result<String> {
    let has_scheme = path
        .get(..7)
        .is_some_and(|s| s.eq_ignore_ascii_case("file://"));
    let local = if has_scheme {
        path.get(7..).unwrap_or("")
    } else {
        path
    };
    let bytes = local.as_bytes();
    let drive = matches!(bytes, [d, b':', b'\\' | b'/', ..] if d.is_ascii_alphabetic());
    if local.contains('\0') || !(drive || local.starts_with('\\') || local.starts_with('/')) {
        return Err(Error::Unsupported(
            "DGN raster path must be absolute".into(),
        ));
    }
    Ok(if has_scheme {
        path.to_owned()
    } else {
        format!("file://{path}")
    })
}

/// `dgn.raster_file`, else the last component of the path.
fn file_name(e: &Entity, path: &str) -> Result<String> {
    let name = match e.props.get("dgn.raster_file") {
        Some(Value::Text(s)) => s.clone(),
        Some(_) => return Err(Error::invalid(0, "dgn.raster_file must be text")),
        None => path.rsplit(['\\', '/']).next().unwrap_or("").to_owned(),
    };
    if name.is_empty() || name.contains(['\\', '/', '\0']) {
        return Err(Error::invalid(
            0,
            "DGN raster file name must be a bare file name",
        ));
    }
    Ok(name)
}

/// The frame geometry of an image in UOR.
struct Placement {
    /// Lower-left corner.
    origin: [f64; 3],
    /// One pixel along the bottom and the left edge.
    a: Vec3,
    b: Vec3,
    /// Unit normal of the image plane.
    n: Vec3,
    /// Edge lengths along `a` and `b` (width·|a|, height·|b|).
    edges: [f64; 2],
}

impl Placement {
    fn new(
        position: Point3,
        u: Vec3,
        v: Vec3,
        [w, h]: [u32; 2],
        model: &ModelHeader,
    ) -> Result<Self> {
        if w == 0 || h == 0 {
            return Err(Error::invalid(0, "DGN raster size_px must be positive"));
        }
        if !model.is_3d && (u.z != 0.0 || v.z != 0.0) {
            return Err(Error::Unsupported(
                "nonzero Z cannot be written into a 2D seed".into(),
            ));
        }
        let origin = coordinate(position, model)?;
        let (u, v) = (
            u.scaled(model.uor_per_master),
            v.scaled(model.uor_per_master),
        );
        let (Some(ua), Some(ub)) = (u.normalized(), v.normalized()) else {
            return Err(Error::invalid(0, "DGN raster edge vectors must be nonzero"));
        };
        let Some(n) = ua
            .cross(ub)
            .normalized()
            .filter(|_| ua.cross(ub).length() > 1e-9)
        else {
            return Err(Error::invalid(0, "DGN raster edge vectors are parallel"));
        };
        let p = Self {
            origin,
            a: u.scaled(1.0 / f64::from(w)),
            b: v.scaled(1.0 / f64::from(h)),
            n,
            edges: [u.length(), v.length()],
        };
        if !p.corners().iter().flatten().all(|c| c.is_finite()) {
            return Err(Error::invalid(0, "DGN raster corner outside UOR range"));
        }
        Ok(p)
    }

    fn u(&self) -> Vec3 {
        self.a.normalized().unwrap_or(self.a).scaled(self.edges[0])
    }

    fn v(&self) -> Vec3 {
        self.b.normalized().unwrap_or(self.b).scaled(self.edges[1])
    }

    fn corners(&self) -> [[f64; 3]; 4] {
        let [x, y, z] = self.origin;
        let (u, v) = (self.u(), self.v());
        [Vec3::new(0.0, 0.0, 0.0), u, v, u.plus(v)].map(|d| [x + d.x, y + d.y, z + d.z])
    }

    /// The 91 placement record (subtype `0x20007`): origin, edge lengths, rotation of the X
    /// axis and skew of the Y axis in [0, 2π), resolution and both pixel axes.
    fn write_settings(&self, b: &mut [u8], dpi: [f64; 2]) -> Result<()> {
        let rotation = self.a.y.atan2(self.a.x);
        let skew = (self.b.y.atan2(self.b.x) - rotation - FRAC_PI_2).rem_euclid(TAU);
        let values = [
            (0x28, self.origin[0]),
            (0x30, self.origin[1]),
            (0x38, self.origin[2]),
            (0x40, self.edges[0]),
            (0x48, self.edges[1]),
            (0x50, rotation.rem_euclid(TAU)),
            (0x58, skew),
            (0x60, dpi[0]),
            (0x68, dpi[1]),
            (0x88, self.a.x),
            (0x90, self.a.y),
            (0x98, self.a.z),
            (0xa0, self.b.x),
            (0xa8, self.b.y),
            (0xb0, self.b.z),
        ];
        for (offset, v) in values {
            f64_at(b, offset, v)?;
        }
        Ok(())
    }
}

#[cfg(test)]
impl Template {
    /// Whether `self` and `other` agree outside the fields `instantiate` writes.
    pub fn same_constants(&self, other: &Self) -> bool {
        fn masked(b: &[u8], ranges: &[(usize, usize)]) -> Vec<u8> {
            let mut b = b.to_vec();
            for &(from, to) in [(0x10, 0x20)].iter().chain(ranges) {
                b.iter_mut().take(to).skip(from).for_each(|x| *x = 0);
            }
            b
        }
        let same =
            |a: &[u8], b: &[u8], ranges: &[(usize, usize)]| masked(a, ranges) == masked(b, ranges);
        let frame = [(0x0c, 0x10), (0x38, 0x68), (0x78, 0xf8), (0x108, 0x118)];
        let placement = [(0x28, 0x70), (0x88, 0xb8)];
        let link = [(0x30, 0x38), (self.link_root, self.link_root + 8)];
        let xattribute = |x: &XAttribute| {
            let subs: Vec<_> = x
                .subs
                .iter()
                .enumerate()
                .map(|(i, (head, data))| {
                    let head = masked(head, &[(8, 10)]);
                    (
                        head,
                        if matches!(i, 3 | 4) {
                            vec![]
                        } else {
                            data.clone()
                        },
                    )
                })
                .collect();
            (x.reserved, x.flags, subs, x.trailer.clone())
        };
        same(&self.frame, &other.frame, &frame)
            && self.link_root == other.link_root
            && same(&self.link, &other.link, &link)
            && self
                .options
                .iter()
                .zip(&other.options)
                .all(|(a, b)| same(a, b, &[]))
            && same(
                &self.reference,
                &other.reference,
                &[(0x04, 0x08), (0x38, 0x40)],
            )
            && same(
                &self.reference_dependency,
                &other.reference_dependency,
                &[(12, 20)],
            )
            && self.placement == other.placement
            && self.settings.len() == other.settings.len()
            && self
                .settings
                .iter()
                .zip(&other.settings)
                .enumerate()
                .all(|(i, (a, b))| same(a, b, if i == self.placement { &placement } else { &[] }))
            && xattribute(&self.xattribute) == xattribute(&other.xattribute)
    }
}

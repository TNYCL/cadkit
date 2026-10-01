//! Object stream framework (ODA spec §20.1–§20.4.2).
//!
//! Every object starts with `MS` size (and, R2010+, `MC` handle-stream size). Its
//! data is split into up to three bit streams:
//!
//! | Stream | R13–R14 | R2000–R2007 | R2010+ |
//! |---|---|---|---|
//! | main data | after `MS` | after `MS` | after `MS` `MC` |
//! | handles | at `RL` bit size read after EED/graphics | at `RL` bit size after the type | `MC` bits before the end |
//! | strings | inline | inline (R2007: separate) | separate |
//!
//! The R2007+ string stream is found backwards from the last bit before the handle
//! stream (see [`locate_string_stream`]).
//!
//! Type-specific decoders live in [`entities`] and [`tables`] and are selected by
//! [`registry::lookup`].

pub mod annotation;
pub mod curves;
pub mod entities;
pub mod hatch;
pub mod misc;
pub mod registry;
pub mod tables;

use cadkit_core::{Error, Result, Warning};
use encoding_rs::Encoding;

use crate::bits::BitReader;
use crate::codepage;
use crate::crc::crc8;
use crate::native::{
    ClassTable, CmColor, CommonData, DwgObject, EedBlock, EedItem, EntityCommon, ObjectData,
};
use crate::version::DwgVersion;

/// Settings shared by all object decoders.
#[derive(Debug, Clone, Copy)]
pub struct DecodeContext {
    /// File version.
    pub version: DwgVersion,
    /// Encoding of 8-bit strings (pre-R2007).
    pub encoding: &'static Encoding,
    /// Longest accepted string in bytes.
    pub max_string: u32,
    /// Most vertices per entity.
    pub max_vertices: u32,
}

/// The bit streams of one object (or of the header section) plus everything a
/// decoder needs to interpret version-dependent codes.
#[derive(Debug, Clone)]
pub struct Streams<'a> {
    /// Main data stream.
    pub main: BitReader<'a>,
    /// R2007+ string stream; `None` when absent (strings then read as empty).
    pub strings: Option<BitReader<'a>>,
    /// Handle stream; `None` means handles are inline in `main` (pre-R2007 header).
    pub handles: Option<BitReader<'a>>,
    /// Decoding settings.
    pub ctx: DecodeContext,
    /// Handle of the object being decoded; reference for relative handle codes.
    pub own_handle: u64,
    /// Recoverable problems found by the decoder (moved to [`DwgObject::warnings`]).
    pub warnings: Vec<Warning>,
}

impl<'a> Streams<'a> {
    /// Streams of one object or section; no handle stream means inline handles.
    pub fn new(
        main: BitReader<'a>,
        strings: Option<BitReader<'a>>,
        handles: Option<BitReader<'a>>,
        ctx: DecodeContext,
    ) -> Self {
        Self {
            main,
            strings,
            handles,
            ctx,
            own_handle: 0,
            warnings: Vec::new(),
        }
    }

    /// File version.
    pub fn version(&self) -> DwgVersion {
        self.ctx.version
    }

    /// Records a recoverable problem of the object being decoded.
    pub fn warn(&mut self, code: &str, message: String) {
        self.warnings.push(Warning {
            code: code.to_owned(),
            message,
            offset: None,
            object: Some(self.own_handle),
        });
    }

    /// `TV`: 8-bit text from the main stream before R2007, UTF-16 text from the
    /// string stream from R2007.
    pub fn tv(&mut self) -> Result<String> {
        let text = if self.ctx.version.r2007_plus() {
            match &mut self.strings {
                Some(s) => s.tu(self.ctx.max_string)?,
                None => String::new(),
            }
        } else {
            let bytes = self.main.t_bytes(self.ctx.max_string)?;
            codepage::decode(&bytes, self.ctx.encoding)
        };
        Ok(codepage::decode_escapes(&text))
    }

    /// Handle from the handle stream, resolved against the object's own handle.
    pub fn h(&mut self) -> Result<u64> {
        let (offset, href) = match &mut self.handles {
            Some(h) => (h.offset(), h.handle()?),
            None => (self.main.offset(), self.main.handle()?),
        };
        href.resolve(self.own_handle)
            .ok_or_else(|| Error::invalid(offset, format!("invalid handle code {:#x}", href.code)))
    }

    /// Like [`Streams::h`], mapping the null handle to `None`.
    pub fn h_opt(&mut self) -> Result<Option<u64>> {
        Ok(Some(self.h()?).filter(|&h| h != 0))
    }

    /// `CMC` color as used by table records and header variables (§2.11): a BS
    /// index before R2004; BS, BL value, RC flags and optional names from R2004.
    pub fn cmc(&mut self) -> Result<CmColor> {
        let index = self.main.bs()?;
        if !self.ctx.version.r2004_plus() {
            return Ok(CmColor {
                index,
                ..CmColor::default()
            });
        }
        let value = self.main.bl()? as u32;
        let flags = self.main.rc()?;
        let name = if flags & 1 != 0 {
            Some(self.tv()?)
        } else {
            None
        };
        let book = if flags & 2 != 0 {
            Some(self.tv()?)
        } else {
            None
        };
        Ok(CmColor {
            index,
            value: Some(value),
            name,
            book,
            ..CmColor::default()
        })
    }

    /// Entity color (`ENC` from R2004, a BS index before, §2.11).
    pub fn enc(&mut self) -> Result<CmColor> {
        let raw = self.main.bs()?;
        if !self.ctx.version.r2004_plus() {
            return Ok(CmColor {
                index: raw,
                ..CmColor::default()
            });
        }
        let flags = (raw as u16) & 0xFF00;
        let mut color = CmColor {
            index: raw & 0x01FF,
            ..CmColor::default()
        };
        if flags & 0x4000 != 0 {
            // A DBCOLOR reference; the color itself lives in that object.
            color.has_book_reference = true;
        } else if flags & 0x8000 != 0 {
            color.value = Some(self.main.bl()? as u32);
        }
        if flags & 0x2000 != 0 {
            color.transparency = Some(self.main.bl()? as u32);
        }
        Ok(color)
    }
}

/// Finds the R2007+ string stream that ends just before `flag_bit` (§20.1): if
/// the bit at `flag_bit` is set, a 16-bit size (in bits) precedes it, optionally
/// extended by a second 16-bit word when bit 15 is set; the string data precedes
/// the size. Returns the bit range of the string data.
pub fn locate_string_stream(
    reader: &BitReader<'_>,
    lower: u64,
    flag_bit: u64,
) -> Result<Option<(u64, u64)>> {
    let mut r = reader.clone();
    r.seek_bit(flag_bit)?;
    if !r.b()? {
        return Ok(None);
    }
    let bad = || {
        Error::invalid(
            reader.offset(),
            "string stream size points before the object",
        )
    };
    let mut end = flag_bit
        .checked_sub(16)
        .filter(|&e| e >= lower)
        .ok_or_else(bad)?;
    r.seek_bit(end)?;
    let lo = r.rs()?;
    let mut size = u64::from(lo);
    if lo & 0x8000 != 0 {
        end = end
            .checked_sub(16)
            .filter(|&e| e >= lower)
            .ok_or_else(bad)?;
        r.seek_bit(end)?;
        let hi = r.rs()?;
        size = u64::from(lo & 0x7FFF) | (u64::from(hi) << 15);
    }
    let start = end
        .checked_sub(size)
        .filter(|&s| s >= lower)
        .ok_or_else(bad)?;
    Ok(Some((start, end)))
}

/// Decodes the items of one EED block (spec chapter 28). Stops at the first
/// unknown code; the raw bytes are kept by the caller either way.
pub fn decode_eed(
    data: &[u8],
    version: DwgVersion,
    encoding: &'static Encoding,
) -> Result<Vec<EedItem>> {
    let mut r = BitReader::new(data);
    let mut items = Vec::new();
    while r.remaining_bits() >= 8 {
        let code = r.rc()?;
        let item = match code {
            0 | 1 => {
                if version.r2007_plus() {
                    let len = r.rs()?;
                    let mut units = Vec::with_capacity(usize::from(len).min(data.len()));
                    for _ in 0..len {
                        units.push(r.rs()?);
                    }
                    EedItem::String(codepage::decode_escapes(&String::from_utf16_lossy(&units)))
                } else {
                    // R13–R2004: RC length, RS code page, then the bytes (§28).
                    let len = r.rc()?;
                    let _codepage = r.rs()?;
                    let bytes = r.bytes(usize::from(len))?;
                    EedItem::String(codepage::decode_escapes(&codepage::decode(
                        &bytes, encoding,
                    )))
                }
            }
            2 => EedItem::Control(r.rc()? != 0),
            3 => EedItem::Layer(u64::from_be_bytes(r.array::<8>()?)),
            4 => {
                let len = r.rc()?;
                EedItem::Binary(r.bytes(usize::from(len))?)
            }
            5 => EedItem::Handle(u64::from_be_bytes(r.array::<8>()?)),
            10..=13 => EedItem::Point(1000 + u16::from(code), r.rd3()?),
            40..=42 => EedItem::Real(1000 + u16::from(code), r.rd()?),
            70 => EedItem::Short(r.rs()? as i16),
            71 => EedItem::Long(r.rl()? as i32),
            _ => {
                return Err(Error::invalid(
                    r.offset(),
                    format!("unknown EED code {}", 1000 + u16::from(code)),
                ));
            }
        };
        items.push(item);
    }
    Ok(items)
}

/// Reads the EED blocks that follow the object handle.
fn read_eed(s: &mut Streams<'_>) -> Result<Vec<EedBlock>> {
    let mut blocks = Vec::new();
    loop {
        let size = s.main.bs()?;
        if size == 0 {
            return Ok(blocks);
        }
        let size = usize::try_from(size).map_err(|_| s.main.invalid("negative EED size"))?;
        let app = s.main.handle()?.value;
        let data = s.main.bytes(size)?;
        let items = decode_eed(&data, s.ctx.version, s.ctx.encoding).unwrap_or_default();
        blocks.push(EedBlock { app, data, items });
    }
}

/// Number of handles a count may claim without exceeding the handle stream
/// (every handle needs at least one byte).
fn check_count(s: &Streams<'_>, count: i32, what: &str) -> Result<usize> {
    let remaining = match &s.handles {
        Some(h) => h.remaining_bits(),
        None => s.main.remaining_bits(),
    };
    usize::try_from(count)
        .ok()
        .filter(|&n| (n as u64).saturating_mul(8) <= remaining)
        .ok_or_else(|| s.main.invalid(format!("{what} count {count}")))
}

/// Checks an item count read from the file against `Limits::max_vertices` and the main
/// stream bits left (`min_bits` per item) before anything is allocated or looped.
pub fn checked_count(s: &Streams<'_>, count: i32, min_bits: u64) -> Result<usize> {
    let n =
        usize::try_from(count).map_err(|_| s.main.invalid(format!("negative count {count}")))?;
    if n as u64 > u64::from(s.ctx.max_vertices) {
        return Err(Error::LimitExceeded(format!("{n} items in one object")));
    }
    if (n as u64).saturating_mul(min_bits) > s.main.remaining_bits() {
        return Err(s.main.invalid(format!("count {n} exceeds the object data")));
    }
    Ok(n)
}

/// Largest accepted spline degree. Real splines use small degrees; an absurd value
/// from a corrupt file would make consumers build enormous basis tables.
pub const MAX_SPLINE_DEGREE: i32 = 25;

/// Clamps a spline degree read from the file to `1..=MAX_SPLINE_DEGREE`, with a
/// warning when it was out of range.
pub fn spline_degree(s: &mut Streams<'_>, degree: i32) -> i32 {
    let clamped = degree.clamp(1, MAX_SPLINE_DEGREE);
    if clamped != degree {
        s.warn(
            "dwg.spline_degree_clamped",
            format!("spline degree {degree} clamped to {clamped}"),
        );
    }
    clamped
}

/// Reads `count` handles after checking they can fit in the handle stream.
pub fn read_handles(s: &mut Streams<'_>, count: i32, what: &str) -> Result<Vec<u64>> {
    let n = usize::try_from(count).map_err(|_| s.main.invalid(format!("negative {what} count")))?;
    let remaining = s.handles.as_ref().map_or(0, |h| h.remaining_bits());
    if (n as u64).saturating_mul(8) > remaining {
        return Err(s
            .main
            .invalid(format!("{n} {what} handles exceed the handle stream")));
    }
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        out.push(s.h()?);
    }
    Ok(out)
}

/// Object layout facts found while reading the prefix and common data.
struct Layout {
    start: u64,
    end: u64,
    handle_start: Option<u64>,
    strings: Option<(u64, u64)>,
}

/// Sets up the handle stream (and, for R2007, the string stream) once the bit size of
/// the main data is known (`RL` "size of object data in bits").
fn attach_handle_stream(s: &mut Streams<'_>, layout: &mut Layout, bit_size: u32) -> Result<()> {
    let handle_start = layout
        .start
        .checked_add(u64::from(bit_size))
        .filter(|&h| h <= layout.end)
        .ok_or_else(|| {
            s.main.invalid(format!(
                "object data size of {bit_size} bits exceeds the object"
            ))
        })?;
    layout.handle_start = Some(handle_start);
    s.handles = Some(s.main.range(handle_start, layout.end));
    if s.ctx.version.r2007_plus() && handle_start > layout.start {
        layout.strings = locate_string_stream(
            &s.main.range(layout.start, layout.end),
            layout.start,
            handle_start - 1,
        )?;
        let main_end = layout.strings.map_or(handle_start, |(start, _)| start);
        s.strings = layout.strings.map(|(a, b)| s.main.range(a, b));
        s.main.set_end_bit(main_end);
    } else {
        s.main.set_end_bit(handle_start);
    }
    Ok(())
}

/// Main-stream part of the reactor block (§20.1): `BL` reactor count, R2004+ "no
/// xdictionary" bit, R2013+ data-store bit. Returns (reactor count, xdic missing).
fn read_reactor_flags(s: &mut Streams<'_>, common: &mut CommonData) -> Result<(usize, bool)> {
    let count = s.main.bl()?;
    let reactors = check_count(s, count, "reactor")?;
    let xdic_missing = if s.ctx.version.r2004_plus() {
        s.main.b()?
    } else {
        false
    };
    if s.ctx.version.r2013_plus() {
        common.has_ds_binary = s.main.b()?;
    }
    Ok((reactors, xdic_missing))
}

/// Handle-stream part of the reactor block: reactor handles, then the xdictionary.
fn read_reactor_handles(
    s: &mut Streams<'_>,
    common: &mut CommonData,
    reactors: usize,
    xdic_missing: bool,
) -> Result<()> {
    for _ in 0..reactors {
        common.reactors.push(s.h()?);
    }
    if !xdic_missing {
        common.xdictionary = s.h_opt()?;
    }
    Ok(())
}

/// Common entity data (§20.4.1) and common entity handles (§20.4.2).
fn read_entity_common(
    s: &mut Streams<'_>,
    layout: &mut Layout,
    common: &mut CommonData,
) -> Result<()> {
    let v = s.ctx.version;
    let mut graphics_size = None;
    if s.main.b()? {
        let size = if v.r2010_plus() {
            s.main.bll()?
        } else {
            u64::from(s.main.rl()?)
        };
        graphics_size = Some(size);
        let bits = size
            .checked_mul(8)
            .ok_or_else(|| s.main.invalid("graphics size"))?;
        s.main.skip_bits(bits)?;
    }
    if v.r13_14() {
        let bit_size = s.main.rl()?;
        attach_handle_stream(s, layout, bit_size)?;
    }
    let mut e = read_entity_mode(s, common)?;
    e.graphics_size = graphics_size;
    common.entity = Some(e);
    Ok(())
}

/// The part of the common entity data that starts at the entity mode (§20.4.1) with
/// its handles (§20.4.2). Also used for the MTEXT embedded in R2018 multi-line
/// attributes, which repeats exactly this part.
pub fn read_entity_mode(s: &mut Streams<'_>, common: &mut CommonData) -> Result<EntityCommon> {
    let v = s.ctx.version;
    let mut e = EntityCommon {
        linetype_scale: 1.0,
        ..EntityCommon::default()
    };
    e.entity_mode = s.main.bb()?;
    let (reactors, xdic_missing) = read_reactor_flags(s, common)?;
    let by_layer_lt = if v.r13_14() { s.main.b()? } else { false };
    if !v.r2004_plus() {
        e.no_links = s.main.b()?;
    } else {
        e.no_links = true;
    }
    e.color = s.enc()?;
    e.linetype_scale = s.main.bd()?;
    if v.r2000_plus() {
        e.linetype_flags = s.main.bb()?;
        e.plotstyle_flags = s.main.bb()?;
        if v.r2007_plus() {
            e.material_flags = s.main.bb()?;
            e.shadow_flags = s.main.rc()?;
        }
    } else {
        e.linetype_flags = if by_layer_lt { 0 } else { 3 };
    }
    let mut visual_styles = [false; 3];
    if v.r2010_plus() {
        for flag in &mut visual_styles {
            *flag = s.main.b()?;
        }
    }
    e.invisible = s.main.bs()? & 1 != 0;
    if v.r2000_plus() {
        e.lineweight = Some(s.main.rc()?);
    }

    // Handle stream.
    if e.entity_mode == 0 {
        common.owner = s.h_opt()?;
    }
    read_reactor_handles(s, common, reactors, xdic_missing)?;
    if v.r13_14() {
        e.layer = s.h_opt()?;
        if !by_layer_lt {
            e.linetype = s.h_opt()?;
        }
    }
    if !v.r2004_plus() && !e.no_links {
        e.prev = s.h_opt()?;
        e.next = s.h_opt()?;
    }
    if v.r2004_plus() && e.color.has_book_reference {
        e.color_book = s.h_opt()?;
    }
    if v.r2000_plus() {
        e.layer = s.h_opt()?;
        if e.linetype_flags == 3 {
            e.linetype = s.h_opt()?;
        }
        if v.r2007_plus() && e.material_flags == 3 {
            e.material = s.h_opt()?;
        }
        if e.plotstyle_flags == 3 {
            e.plotstyle = s.h_opt()?;
        }
        for (slot, present) in e.visual_styles.iter_mut().zip(visual_styles) {
            if present {
                *slot = s.h_opt()?;
            }
        }
    }
    Ok(e)
}

/// Common non-entity data (§20.1).
fn read_object_common(
    s: &mut Streams<'_>,
    layout: &mut Layout,
    common: &mut CommonData,
) -> Result<()> {
    if s.ctx.version.r13_14() {
        let bit_size = s.main.rl()?;
        attach_handle_stream(s, layout, bit_size)?;
    }
    let (reactors, xdic_missing) = read_reactor_flags(s, common)?;
    common.owner = s.h_opt()?;
    read_reactor_handles(s, common, reactors, xdic_missing)
}

/// Parses the object at `offset` of the object stream `stream` (whose first byte is
/// at file/section offset `base`).
pub fn parse_object(
    stream: &[u8],
    base: u64,
    offset: u64,
    ctx: DecodeContext,
    classes: &ClassTable<'_>,
    keep_raw: bool,
) -> Result<DwgObject> {
    let v = ctx.version;
    let mut r = BitReader::with_base(stream, base);
    let start_bit = offset
        .checked_mul(8)
        .ok_or_else(|| Error::invalid(base, "object offset overflow"))?;
    r.seek_bit(start_bit)?;
    let size = r.ms()?;
    let handle_bits = if v.r2010_plus() { Some(r.umc()?) } else { None };
    let start = r.bit_pos();
    let end = size
        .checked_mul(8)
        .and_then(|b| start.checked_add(b))
        .filter(|&e| e <= r.end_bit())
        .ok_or_else(|| {
            Error::invalid(
                base.saturating_add(offset),
                format!("object size {size} exceeds the stream"),
            )
        })?;

    // CRC over size prefix and data (§20.1), stored right after the data.
    let crc_ok = {
        let (from, to) = (usize::try_from(offset).ok(), usize::try_from(end / 8).ok());
        match (from, to) {
            (Some(from), Some(to)) => {
                match (stream.get(from..to), stream.get(to..to.saturating_add(2))) {
                    (Some(body), Some(&[lo, hi])) => {
                        Some(crc8(0xC0C1, body) == u16::from_le_bytes([lo, hi]))
                    }
                    _ => None,
                }
            }
            _ => None,
        }
    };

    let mut main = r.range(start, end);
    let type_code = main.ot(v.r2010_plus())?;
    let info = registry::lookup(type_code, classes);
    let mut s = Streams::new(main, None, None, ctx);
    let mut layout = Layout {
        start,
        end,
        handle_start: None,
        strings: None,
    };

    if let Some(hbits) = handle_bits {
        let handle_start = end
            .checked_sub(hbits)
            .filter(|&h| h >= start)
            .ok_or_else(|| {
                s.main
                    .invalid(format!("handle stream of {hbits} bits exceeds the object"))
            })?;
        let bit_size =
            u32::try_from(handle_start - start).map_err(|_| s.main.invalid("object too large"))?;
        attach_handle_stream(&mut s, &mut layout, bit_size)?;
    } else if v.r2000_plus() {
        let bit_size = s.main.rl()?;
        attach_handle_stream(&mut s, &mut layout, bit_size)?;
    }

    let own = s.main.handle()?;
    s.own_handle = own.value;
    let mut common = CommonData {
        eed: read_eed(&mut s)?,
        ..CommonData::default()
    };
    if info.entity {
        read_entity_common(&mut s, &mut layout, &mut common)?;
    } else {
        read_object_common(&mut s, &mut layout, &mut common)?;
    }

    let (data, error, data_bits_left, handle_bits_left) = match info.decoder {
        Some(decode) => match decode(&mut s) {
            Ok(data) => (
                data,
                None,
                Some(s.main.remaining_bits()),
                s.handles.as_ref().map(BitReader::remaining_bits),
            ),
            Err(e) => (ObjectData::Unparsed, Some(e.to_string()), None, None),
        },
        None => (ObjectData::Unparsed, None, None, None),
    };

    let raw = if keep_raw {
        let (a, b) = (
            usize::try_from(start / 8).unwrap_or(usize::MAX),
            usize::try_from(end / 8).unwrap_or(usize::MAX),
        );
        stream.get(a..b).map(<[u8]>::to_vec).unwrap_or_default()
    } else {
        Vec::new()
    };
    Ok(DwgObject {
        handle: own.value,
        type_code,
        type_name: info.name,
        is_entity: info.entity,
        offset,
        size,
        crc_ok,
        common,
        data,
        raw,
        handle_stream_bit: layout.handle_start.map_or(0, |h| h - start),
        string_stream: layout.strings.map(|(a, b)| (a - start, b - start)),
        error,
        warnings: s.warnings,
        data_bits_left,
        handle_bits_left,
    })
}

/// Builds bit streams for decoder tests, in the reader's bit order (MSB first, raw
/// multi-byte values little-endian).
#[cfg(test)]
pub(crate) mod testbits {
    use super::{DecodeContext, Streams};
    use crate::bits::BitReader;
    use crate::version::DwgVersion;

    #[derive(Debug, Default)]
    pub(crate) struct BitWriter {
        bytes: Vec<u8>,
        bits: usize,
    }

    impl BitWriter {
        pub(crate) fn bit(&mut self, b: bool) -> &mut Self {
            if self.bits % 8 == 0 {
                self.bytes.push(0);
            }
            if b {
                let i = self.bits / 8;
                self.bytes[i] |= 0x80 >> (self.bits % 8);
            }
            self.bits += 1;
            self
        }

        pub(crate) fn bits(&mut self, value: u64, n: u32) -> &mut Self {
            for i in (0..n).rev() {
                self.bit((value >> i) & 1 != 0);
            }
            self
        }

        pub(crate) fn rc(&mut self, v: u8) -> &mut Self {
            self.bits(u64::from(v), 8)
        }

        fn raw(&mut self, bytes: &[u8]) -> &mut Self {
            for &b in bytes {
                self.rc(b);
            }
            self
        }

        pub(crate) fn bs(&mut self, v: i16) -> &mut Self {
            match v {
                0 => self.bits(0b10, 2),
                256 => self.bits(0b11, 2),
                1..=255 => self.bits(0b01, 2).rc(v as u8),
                _ => self.bits(0b00, 2).raw(&v.to_le_bytes()),
            }
        }

        pub(crate) fn bl(&mut self, v: i32) -> &mut Self {
            match v {
                0 => self.bits(0b10, 2),
                1..=255 => self.bits(0b01, 2).rc(v as u8),
                _ => self.bits(0b00, 2).raw(&v.to_le_bytes()),
            }
        }

        pub(crate) fn bd(&mut self, v: f64) -> &mut Self {
            if v == 0.0 {
                self.bits(0b10, 2)
            } else if v == 1.0 {
                self.bits(0b01, 2)
            } else {
                self.bits(0b00, 2).raw(&v.to_le_bytes())
            }
        }

        pub(crate) fn rd(&mut self, v: f64) -> &mut Self {
            self.raw(&v.to_le_bytes())
        }

        pub(crate) fn bytes(&self) -> Vec<u8> {
            self.bytes.clone()
        }
    }

    /// A decoding context for `version` with the given vertex limit.
    pub(crate) fn ctx(version: DwgVersion, max_vertices: u32) -> DecodeContext {
        DecodeContext {
            version,
            encoding: encoding_rs::WINDOWS_1252,
            max_string: 1 << 16,
            max_vertices,
        }
    }

    /// Streams over `data` with an empty handle stream.
    pub(crate) fn streams(data: &[u8], ctx: DecodeContext) -> Streams<'_> {
        Streams::new(BitReader::new(data), None, Some(BitReader::new(&[])), ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spline_degree_is_clamped_with_a_warning() {
        let data = [0u8; 1];
        let mut s = testbits::streams(&data, testbits::ctx(DwgVersion::R2000, 100));
        assert_eq!(spline_degree(&mut s, 3), 3);
        assert!(s.warnings.is_empty());
        assert_eq!(spline_degree(&mut s, 1_000_000), MAX_SPLINE_DEGREE);
        assert_eq!(spline_degree(&mut s, -4), 1);
        assert_eq!(s.warnings.len(), 2);
        assert_eq!(s.warnings[0].code, "dwg.spline_degree_clamped");
    }

    #[test]
    fn string_stream_location() {
        // 4 bytes of data; string stream = bits 0..16, size word 16 at bits 16..32,
        // flag bit set at bit 32.
        let mut data = vec![0xAA, 0xBB];
        data.extend(16u16.to_le_bytes());
        data.push(0x80);
        let r = BitReader::new(&data);
        assert_eq!(locate_string_stream(&r, 0, 32).unwrap(), Some((0, 16)));
        // Flag clear.
        let data = [0u8; 5];
        assert_eq!(
            locate_string_stream(&BitReader::new(&data), 0, 32).unwrap(),
            None
        );
        // Size pointing before the lower bound.
        let mut data = vec![0xAA, 0xBB];
        data.extend(100u16.to_le_bytes());
        data.push(0x80);
        assert!(locate_string_stream(&BitReader::new(&data), 0, 32).is_err());
    }

    #[test]
    fn eed_items_pre_2007() {
        let mut data = vec![2, 0]; // '{'
        data.extend([0, 3, 30, 0, b'a', b'b', b'c']); // string, len 3, codepage 30
        data.push(70);
        data.extend(7i16.to_le_bytes());
        data.push(40);
        data.extend(1.5f64.to_le_bytes());
        data.extend([2, 1]); // '}'
        let items = decode_eed(&data, DwgVersion::R2000, encoding_rs::WINDOWS_1252).unwrap();
        assert_eq!(
            items,
            vec![
                EedItem::Control(false),
                EedItem::String("abc".into()),
                EedItem::Short(7),
                EedItem::Real(1040, 1.5),
                EedItem::Control(true)
            ]
        );
        assert!(decode_eed(&[99], DwgVersion::R2000, encoding_rs::WINDOWS_1252).is_err());
    }

    #[test]
    fn eed_string_r2007() {
        let mut data = vec![0];
        data.extend(2u16.to_le_bytes());
        data.extend([b'h', 0, b'i', 0]);
        let items = decode_eed(&data, DwgVersion::R2010, encoding_rs::WINDOWS_1252).unwrap();
        assert_eq!(items, vec![EedItem::String("hi".into())]);
    }
}

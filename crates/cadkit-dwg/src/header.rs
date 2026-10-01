//! Header variables section (ODA spec chapter 9).
//!
//! The variables are a long version-dependent bit stream. They are decoded in
//! spec order into [`HeaderVars`] by name; fields the spec calls "unknown" are
//! skipped. From R2007 strings come from a string stream and handles (except
//! HANDSEED) from a handle stream at the end of the section, like objects.

use std::collections::BTreeMap;

use cadkit_core::{Error, Result, Value};

use crate::bits::BitReader;
use crate::container::{Section, sentinels};
use crate::crc::crc8;
use crate::native::{CmColor, HeaderVars};
use crate::object::{DecodeContext, Streams, locate_string_stream};
use crate::version::DwgVersion;

struct Parser<'a> {
    s: Streams<'a>,
    vars: Vec<(String, Value)>,
    handles: BTreeMap<String, u64>,
}

fn color_value(c: &CmColor) -> Value {
    match c.value {
        Some(v) => Value::List(vec![
            Value::Int(i64::from(c.index)),
            Value::Int(i64::from(v)),
        ]),
        None => Value::Int(i64::from(c.index)),
    }
}

impl Parser<'_> {
    fn v(&self) -> DwgVersion {
        self.s.ctx.version
    }
    fn put(&mut self, name: &str, value: Value) {
        self.vars.push((name.to_owned(), value));
    }
    fn b(&mut self, name: &str) -> Result<()> {
        let v = self.s.main.b()?;
        self.put(name, Value::Bool(v));
        Ok(())
    }
    fn bs(&mut self, name: &str) -> Result<()> {
        let v = self.s.main.bs()?;
        self.put(name, Value::Int(i64::from(v)));
        Ok(())
    }
    fn bl(&mut self, name: &str) -> Result<()> {
        let v = self.s.main.bl()?;
        self.put(name, Value::Int(i64::from(v)));
        Ok(())
    }
    fn rc(&mut self, name: &str) -> Result<()> {
        let v = self.s.main.rc()?;
        self.put(name, Value::Int(i64::from(v)));
        Ok(())
    }
    fn bd(&mut self, name: &str) -> Result<()> {
        let v = self.s.main.bd()?;
        self.put(name, Value::Float(v));
        Ok(())
    }
    fn bd3(&mut self, name: &str) -> Result<()> {
        let [x, y, z] = self.s.main.bd3()?;
        self.put(
            name,
            Value::List(vec![Value::Float(x), Value::Float(y), Value::Float(z)]),
        );
        Ok(())
    }
    fn rd2(&mut self, name: &str) -> Result<()> {
        let [x, y] = self.s.main.rd2()?;
        self.put(name, Value::List(vec![Value::Float(x), Value::Float(y)]));
        Ok(())
    }
    fn tv(&mut self, name: &str) -> Result<()> {
        let v = self.s.tv()?;
        self.put(name, Value::Text(v));
        Ok(())
    }
    fn h(&mut self, name: &str) -> Result<()> {
        let v = self.s.h()?;
        self.handles.insert(name.to_owned(), v);
        self.put(name, Value::Int(v as i64));
        Ok(())
    }
    fn cmc(&mut self, name: &str) -> Result<()> {
        let c = self.s.cmc()?;
        self.put(name, color_value(&c));
        Ok(())
    }
    fn date(&mut self, name: &str) -> Result<()> {
        let day = self.s.main.bl()?;
        let ms = self.s.main.bl()?;
        self.put(
            name,
            Value::List(vec![Value::Int(i64::from(day)), Value::Int(i64::from(ms))]),
        );
        Ok(())
    }
    fn many(&mut self, f: fn(&mut Self, &str) -> Result<()>, names: &[&str]) -> Result<()> {
        for name in names {
            f(self, name)?;
        }
        Ok(())
    }
    fn skip_bd(&mut self, n: usize) -> Result<()> {
        for _ in 0..n {
            self.s.main.bd()?;
        }
        Ok(())
    }
    fn skip_bl(&mut self, n: usize) -> Result<()> {
        for _ in 0..n {
            self.s.main.bl()?;
        }
        Ok(())
    }
    fn skip_tv(&mut self, n: usize) -> Result<()> {
        for _ in 0..n {
            self.s.tv()?;
        }
        Ok(())
    }

    /// The variables of §9 in order.
    fn run(&mut self) -> Result<()> {
        let v = self.v();
        if v.r2013_plus() {
            let req = self.s.main.bll()?;
            self.put("REQUIREDVERSIONS", Value::Int(req as i64));
        }
        self.skip_bd(4)?;
        self.skip_tv(4)?;
        self.skip_bl(2)?;
        if v.r13_14() {
            self.s.main.bs()?;
        }
        if !v.r2004_plus() {
            self.h("CURRENT_VIEWPORT_ENTITY_HEADER")?;
        }
        self.many(Self::b, &["DIMASO", "DIMSHO"])?;
        if v.r13_14() {
            self.b("DIMSAV")?;
        }
        self.many(
            Self::b,
            &[
                "PLINEGEN",
                "ORTHOMODE",
                "REGENMODE",
                "FILLMODE",
                "QTEXTMODE",
                "PSLTSCALE",
                "LIMCHECK",
            ],
        )?;
        if v.r13_14() {
            self.b("BLIPMODE")?;
        }
        if v.r2004_plus() {
            self.s.main.b()?;
        }
        self.many(Self::b, &["USRTIMER", "SKPOLY", "ANGDIR", "SPLFRAME"])?;
        if v.r13_14() {
            self.many(Self::b, &["ATTREQ", "ATTDIA"])?;
        }
        self.many(Self::b, &["MIRRTEXT", "WORLDVIEW"])?;
        if v.r13_14() {
            self.b("WIREFRAME")?;
        }
        self.many(Self::b, &["TILEMODE", "PLIMCHECK", "VISRETAIN"])?;
        if v.r13_14() {
            self.b("DELOBJ")?;
        }
        self.many(Self::b, &["DISPSILH", "PELLIPSE"])?;
        self.bs("PROXYGRAPHICS")?;
        if v.r13_14() {
            self.bs("DRAGMODE")?;
        }
        self.many(
            Self::bs,
            &["TREEDEPTH", "LUNITS", "LUPREC", "AUNITS", "AUPREC"],
        )?;
        if v.r13_14() {
            self.bs("OSMODE")?;
        }
        self.bs("ATTMODE")?;
        if v.r13_14() {
            self.bs("COORDS")?;
        }
        self.bs("PDMODE")?;
        if v.r13_14() {
            self.bs("PICKSTYLE")?;
        }
        if v.r2004_plus() {
            self.skip_bl(3)?;
        }
        self.many(
            Self::bs,
            &[
                "USERI1",
                "USERI2",
                "USERI3",
                "USERI4",
                "USERI5",
                "SPLINESEGS",
                "SURFU",
                "SURFV",
                "SURFTYPE",
                "SURFTAB1",
                "SURFTAB2",
                "SPLINETYPE",
                "SHADEDGE",
                "SHADEDIF",
                "UNITMODE",
                "MAXACTVP",
                "ISOLINES",
                "CMLJUST",
                "TEXTQLTY",
            ],
        )?;
        self.many(
            Self::bd,
            &[
                "LTSCALE",
                "TEXTSIZE",
                "TRACEWID",
                "SKETCHINC",
                "FILLETRAD",
                "THICKNESS",
                "ANGBASE",
                "PDSIZE",
                "PLINEWID",
                "USERR1",
                "USERR2",
                "USERR3",
                "USERR4",
                "USERR5",
                "CHAMFERA",
                "CHAMFERB",
                "CHAMFERC",
                "CHAMFERD",
                "FACETRES",
                "CMLSCALE",
                "CELTSCALE",
            ],
        )?;
        // The spec lists MENUNAME for R13–R2004 only, but R2007+ files store it too
        // (in the string stream); without it every later string is shifted.
        self.tv("MENUNAME")?;
        self.many(Self::date, &["TDCREATE", "TDUPDATE"])?;
        if v.r2004_plus() {
            self.skip_bl(3)?;
        }
        self.many(Self::date, &["TDINDWG", "TDUSRTIMER"])?;
        self.cmc("CECOLOR")?;
        // HANDSEED is in the main stream even when handles are split (§9).
        let seed = self.s.main.handle()?.value;
        self.handles.insert("HANDSEED".into(), seed);
        self.put("HANDSEED", Value::Int(seed as i64));
        self.many(Self::h, &["CLAYER", "TEXTSTYLE", "CELTYPE"])?;
        if v.r2007_plus() {
            self.h("CMATERIAL")?;
        }
        self.many(Self::h, &["DIMSTYLE", "CMLSTYLE"])?;
        if v.r2000_plus() {
            self.bd("PSVPSCALE")?;
        }
        self.many(Self::bd3, &["PINSBASE", "PEXTMIN", "PEXTMAX"])?;
        self.many(Self::rd2, &["PLIMMIN", "PLIMMAX"])?;
        self.bd("PELEVATION")?;
        self.many(Self::bd3, &["PUCSORG", "PUCSXDIR", "PUCSYDIR"])?;
        self.h("PUCSNAME")?;
        if v.r2000_plus() {
            self.h("PUCSORTHOREF")?;
            self.bs("PUCSORTHOVIEW")?;
            self.h("PUCSBASE")?;
            self.many(
                Self::bd3,
                &[
                    "PUCSORGTOP",
                    "PUCSORGBOTTOM",
                    "PUCSORGLEFT",
                    "PUCSORGRIGHT",
                    "PUCSORGFRONT",
                    "PUCSORGBACK",
                ],
            )?;
        }
        self.many(Self::bd3, &["INSBASE", "EXTMIN", "EXTMAX"])?;
        self.many(Self::rd2, &["LIMMIN", "LIMMAX"])?;
        self.bd("ELEVATION")?;
        self.many(Self::bd3, &["UCSORG", "UCSXDIR", "UCSYDIR"])?;
        self.h("UCSNAME")?;
        if v.r2000_plus() {
            self.h("UCSORTHOREF")?;
            self.bs("UCSORTHOVIEW")?;
            self.h("UCSBASE")?;
            self.many(
                Self::bd3,
                &[
                    "UCSORGTOP",
                    "UCSORGBOTTOM",
                    "UCSORGLEFT",
                    "UCSORGRIGHT",
                    "UCSORGFRONT",
                    "UCSORGBACK",
                ],
            )?;
            self.many(Self::tv, &["DIMPOST", "DIMAPOST"])?;
        }
        if v.r13_14() {
            self.many(
                Self::b,
                &[
                    "DIMTOL", "DIMLIM", "DIMTIH", "DIMTOH", "DIMSE1", "DIMSE2", "DIMALT",
                    "DIMTOFL", "DIMSAH", "DIMTIX", "DIMSOXD",
                ],
            )?;
            self.many(Self::rc, &["DIMALTD", "DIMZIN"])?;
            self.many(Self::b, &["DIMSD1", "DIMSD2"])?;
            self.many(Self::rc, &["DIMTOLJ", "DIMJUST", "DIMFIT"])?;
            self.b("DIMUPT")?;
            self.many(Self::rc, &["DIMTZIN", "DIMALTZ", "DIMALTTZ", "DIMTAD"])?;
            self.many(
                Self::bs,
                &[
                    "DIMUNIT", "DIMAUNIT", "DIMDEC", "DIMTDEC", "DIMALTU", "DIMALTTD",
                ],
            )?;
            self.h("DIMTXSTY")?;
        }
        self.many(
            Self::bd,
            &[
                "DIMSCALE", "DIMASZ", "DIMEXO", "DIMDLI", "DIMEXE", "DIMRND", "DIMDLE", "DIMTP",
                "DIMTM",
            ],
        )?;
        if v.r2007_plus() {
            self.many(Self::bd, &["DIMFXL", "DIMJOGANG"])?;
            self.bs("DIMTFILL")?;
            self.cmc("DIMTFILLCLR")?;
        }
        if v.r2000_plus() {
            self.many(
                Self::b,
                &["DIMTOL", "DIMLIM", "DIMTIH", "DIMTOH", "DIMSE1", "DIMSE2"],
            )?;
            self.many(Self::bs, &["DIMTAD", "DIMZIN", "DIMAZIN"])?;
        }
        if v.r2007_plus() {
            self.bs("DIMARCSYM")?;
        }
        self.many(
            Self::bd,
            &[
                "DIMTXT", "DIMCEN", "DIMTSZ", "DIMALTF", "DIMLFAC", "DIMTVP", "DIMTFAC", "DIMGAP",
            ],
        )?;
        if v.r13_14() {
            self.many(
                Self::tv,
                &["DIMPOST", "DIMAPOST", "DIMBLK", "DIMBLK1", "DIMBLK2"],
            )?;
        }
        if v.r2000_plus() {
            self.bd("DIMALTRND")?;
            self.b("DIMALT")?;
            self.bs("DIMALTD")?;
            self.many(Self::b, &["DIMTOFL", "DIMSAH", "DIMTIX", "DIMSOXD"])?;
        }
        self.many(Self::cmc, &["DIMCLRD", "DIMCLRE", "DIMCLRT"])?;
        if v.r2000_plus() {
            self.many(
                Self::bs,
                &[
                    "DIMADEC", "DIMDEC", "DIMTDEC", "DIMALTU", "DIMALTTD", "DIMAUNIT", "DIMFRAC",
                    "DIMLUNIT", "DIMDSEP", "DIMTMOVE", "DIMJUST",
                ],
            )?;
            self.many(Self::b, &["DIMSD1", "DIMSD2"])?;
            self.many(Self::bs, &["DIMTOLJ", "DIMTZIN", "DIMALTZ", "DIMALTTZ"])?;
            self.b("DIMUPT")?;
            self.bs("DIMATFIT")?;
        }
        if v.r2007_plus() {
            self.b("DIMFXLON")?;
        }
        if v.r2010_plus() {
            self.b("DIMTXTDIRECTION")?;
            self.bd("DIMALTMZF")?;
            self.tv("DIMALTMZS")?;
            self.bd("DIMMZF")?;
            self.tv("DIMMZS")?;
        }
        if v.r2000_plus() {
            self.many(
                Self::h,
                &["DIMTXSTY", "DIMLDRBLK", "DIMBLK", "DIMBLK1", "DIMBLK2"],
            )?;
        }
        if v.r2007_plus() {
            self.many(Self::h, &["DIMLTYPE", "DIMLTEX1", "DIMLTEX2"])?;
        }
        if v.r2000_plus() {
            self.many(Self::bs, &["DIMLWD", "DIMLWE"])?;
        }
        self.many(
            Self::h,
            &[
                "BLOCK_CONTROL_OBJECT",
                "LAYER_CONTROL_OBJECT",
                "STYLE_CONTROL_OBJECT",
                "LINETYPE_CONTROL_OBJECT",
                "VIEW_CONTROL_OBJECT",
                "UCS_CONTROL_OBJECT",
                "VPORT_CONTROL_OBJECT",
                "APPID_CONTROL_OBJECT",
                "DIMSTYLE_CONTROL_OBJECT",
            ],
        )?;
        if !v.r2004_plus() {
            self.h("VIEWPORT_ENTITY_HEADER_CONTROL_OBJECT")?;
        }
        self.many(
            Self::h,
            &[
                "DICTIONARY_ACAD_GROUP",
                "DICTIONARY_ACAD_MLINESTYLE",
                "DICTIONARY_NAMED_OBJECTS",
            ],
        )?;
        if v.r2000_plus() {
            self.many(Self::bs, &["TSTACKALIGN", "TSTACKSIZE"])?;
            self.many(Self::tv, &["HYPERLINKBASE", "STYLESHEET"])?;
            self.many(
                Self::h,
                &[
                    "DICTIONARY_LAYOUTS",
                    "DICTIONARY_PLOTSETTINGS",
                    "DICTIONARY_PLOTSTYLES",
                ],
            )?;
        }
        if v.r2004_plus() {
            self.many(Self::h, &["DICTIONARY_MATERIALS", "DICTIONARY_COLORS"])?;
        }
        if v.r2007_plus() {
            self.h("DICTIONARY_VISUALSTYLE")?;
        }
        if v.r2013_plus() {
            self.h("UNKNOWN_DICTIONARY_R2013")?;
        }
        if v.r2000_plus() {
            let flags = self.s.main.bl()?;
            self.put("FLAGS", Value::Int(i64::from(flags)));
            self.put("CELWEIGHT", Value::Int(i64::from(flags & 0x1F)));
            self.put("LWDISPLAY", Value::Bool(flags & 0x200 == 0));
            self.put("EXTNAMES", Value::Bool(flags & 0x800 != 0));
            self.put("PSTYLEMODE", Value::Bool(flags & 0x2000 != 0));
            self.bs("INSUNITS")?;
            let cepsntype = self.s.main.bs()?;
            self.put("CEPSNTYPE", Value::Int(i64::from(cepsntype)));
            if cepsntype == 3 {
                self.h("CPSNID")?;
            }
            self.many(Self::tv, &["FINGERPRINTGUID", "VERSIONGUID"])?;
        }
        if v.r2004_plus() {
            self.many(
                Self::rc,
                &[
                    "SORTENTS",
                    "INDEXCTL",
                    "HIDETEXT",
                    "XCLIPFRAME",
                    "DIMASSOC",
                    "HALOGAP",
                ],
            )?;
            self.many(Self::bs, &["OBSCUREDCOLOR", "INTERSECTIONCOLOR"])?;
            self.many(Self::rc, &["OBSCUREDLTYPE", "INTERSECTIONDISPLAY"])?;
            self.tv("PROJECTNAME")?;
        }
        self.many(
            Self::h,
            &[
                "BLOCK_RECORD_PAPER_SPACE",
                "BLOCK_RECORD_MODEL_SPACE",
                "LTYPE_BYLAYER",
                "LTYPE_BYBLOCK",
                "LTYPE_CONTINUOUS",
            ],
        )?;
        if v.r2007_plus() {
            self.b("CAMERADISPLAY")?;
            self.skip_bl(2)?;
            self.skip_bd(1)?;
            self.many(
                Self::bd,
                &[
                    "STEPSPERSEC",
                    "STEPSIZE",
                    "3DDWFPREC",
                    "LENSLENGTH",
                    "CAMERAHEIGHT",
                ],
            )?;
            self.many(Self::rc, &["SOLIDHIST", "SHOWHIST"])?;
            self.many(
                Self::bd,
                &[
                    "PSOLWIDTH",
                    "PSOLHEIGHT",
                    "LOFTANG1",
                    "LOFTANG2",
                    "LOFTMAG1",
                    "LOFTMAG2",
                ],
            )?;
            self.bs("LOFTPARAM")?;
            self.rc("LOFTNORMALS")?;
            self.many(Self::bd, &["LATITUDE", "LONGITUDE", "NORTHDIRECTION"])?;
            self.bl("TIMEZONE")?;
            self.many(
                Self::rc,
                &[
                    "LIGHTGLYPHDISPLAY",
                    "TILEMODELIGHTSYNCH",
                    "DWFFRAME",
                    "DGNFRAME",
                ],
            )?;
            self.s.main.b()?;
            self.cmc("INTERFERECOLOR")?;
            self.many(Self::h, &["INTERFEREOBJVS", "INTERFEREVPVS", "DRAGVS"])?;
            self.rc("CSHADOW")?;
            self.bd("SHADOWPLANELOCATION")?;
        }
        Ok(())
    }
}

/// Decodes the header variables section. On a decode error the variables read so
/// far are returned with `complete == false` and the error message.
pub fn parse(
    section: &Section<'_>,
    maintenance: u8,
    ctx: DecodeContext,
) -> Result<(HeaderVars, Option<String>)> {
    let v = ctx.version;
    let data: &[u8] = &section.data;
    let mut r = BitReader::with_base(data, section.base);
    r.sentinel(&sentinels::HEADER_START)?;
    let size = u64::from(r.rl()?);
    if (v.r2010_plus() && maintenance > 3) || v.r2018_plus() {
        let _high = r.rl()?;
    }
    let start = r.bit_pos();
    let end = size
        .checked_mul(8)
        .and_then(|b| start.checked_add(b))
        .filter(|&e| e <= r.end_bit())
        .ok_or_else(|| {
            Error::invalid(
                section.base.saturating_add(16),
                format!("header size {size} exceeds the section"),
            )
        })?;
    let mut main = r.range(start, end);
    let mut strings = None;
    let mut handles = None;
    if v.r2007_plus() {
        let bits = u64::from(main.rl()?);
        let flag = start
            .checked_add(bits)
            .and_then(|b| b.checked_sub(1))
            .filter(|&f| f < end && f > start);
        let flag = flag.ok_or_else(|| main.invalid(format!("header bit size {bits}")))?;
        let located = locate_string_stream(&main, start, flag)?;
        handles = Some(main.range(flag + 1, end));
        strings = located.map(|(a, b)| main.range(a, b));
        main.set_end_bit(located.map_or(flag, |(a, _)| a));
    }

    let crc_ok = trailer_ok(data, end, &sentinels::HEADER_END);

    let mut p = Parser {
        s: Streams::new(main, strings, handles, ctx),
        vars: Vec::new(),
        handles: BTreeMap::new(),
    };
    let result = p.run();
    let error = result.err().map(|e| e.to_string());
    let vars = HeaderVars {
        vars: p.vars,
        handles: p.handles,
        complete: error.is_none(),
        crc_ok,
    };
    Ok((vars, error))
}

/// Checks the trailer of a header/classes section: the CRC (seed 0xC0C1) of all
/// bytes after the start sentinel up to the data end `end_bit`, stored as `RS` right
/// after the data, then the end sentinel (§9, §10).
pub fn trailer_ok(data: &[u8], end_bit: u64, end_sentinel: &[u8; 16]) -> Option<bool> {
    let end = usize::try_from(end_bit.div_ceil(8)).ok()?;
    let body = data.get(16..end)?;
    let (lo, hi) = (*data.get(end)?, *data.get(end + 1)?);
    let sentinel = data.get(end + 2..end + 18)?;
    Some(crc8(0xC0C1, body) == u16::from_le_bytes([lo, hi]) && sentinel == end_sentinel)
}

/// MEASUREMENT from a template section (§22): RS description length, description,
/// RS value.
pub fn parse_template(section: &Section<'_>) -> Option<u16> {
    let data: &[u8] = &section.data;
    let len = usize::from(u16::from_le_bytes([*data.first()?, *data.get(1)?]));
    let at = 2usize.checked_add(len)?;
    Some(u16::from_le_bytes([*data.get(at)?, *data.get(at + 1)?]))
}

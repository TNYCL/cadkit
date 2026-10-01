//! DWG file format versions.

/// A DWG file format version, identified by the 6-byte magic at offset 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub enum DwgVersion {
    /// `AC1012`, release 13 (R13).
    R13,
    /// `AC1014`, release 14 (R14).
    R14,
    /// `AC1015`, the 2000 format.
    R2000,
    /// `AC1018`, the 2004 format.
    R2004,
    /// `AC1021`, the 2007 format.
    R2007,
    /// `AC1024`, the 2010 format.
    R2010,
    /// `AC1027`, the 2013 format.
    R2013,
    /// `AC1032`, the 2018 format.
    R2018,
}

impl DwgVersion {
    /// Parses the 6-byte magic (`"AC1015"` …).
    pub fn from_magic(magic: &[u8]) -> Option<Self> {
        Some(match magic {
            b"AC1012" => Self::R13,
            b"AC1014" => Self::R14,
            b"AC1015" => Self::R2000,
            b"AC1018" => Self::R2004,
            b"AC1021" => Self::R2007,
            b"AC1024" => Self::R2010,
            b"AC1027" => Self::R2013,
            b"AC1032" => Self::R2018,
            _ => return None,
        })
    }

    /// The magic string as written in the file.
    pub fn magic(self) -> &'static str {
        match self {
            Self::R13 => "AC1012",
            Self::R14 => "AC1014",
            Self::R2000 => "AC1015",
            Self::R2004 => "AC1018",
            Self::R2007 => "AC1021",
            Self::R2010 => "AC1024",
            Self::R2013 => "AC1027",
            Self::R2018 => "AC1032",
        }
    }

    /// R13 or R14 (the "R13-R14 Only" rows of the spec).
    pub fn r13_14(self) -> bool {
        self <= Self::R14
    }

    /// R2000 and later.
    pub fn r2000_plus(self) -> bool {
        self >= Self::R2000
    }

    /// R2004 and later.
    pub fn r2004_plus(self) -> bool {
        self >= Self::R2004
    }

    /// R2007 and later: UTF-16 strings in a separate string stream.
    pub fn r2007_plus(self) -> bool {
        self >= Self::R2007
    }

    /// R2010 and later: handle stream size prefix and `OT` object types.
    pub fn r2010_plus(self) -> bool {
        self >= Self::R2010
    }

    /// R2013 and later.
    pub fn r2013_plus(self) -> bool {
        self >= Self::R2013
    }

    /// R2018 and later.
    pub fn r2018_plus(self) -> bool {
        self >= Self::R2018
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magic_round_trip_and_order() {
        for v in [
            DwgVersion::R13,
            DwgVersion::R14,
            DwgVersion::R2000,
            DwgVersion::R2004,
            DwgVersion::R2007,
            DwgVersion::R2010,
            DwgVersion::R2013,
            DwgVersion::R2018,
        ] {
            assert_eq!(DwgVersion::from_magic(v.magic().as_bytes()), Some(v));
        }
        assert_eq!(DwgVersion::from_magic(b"AC1009"), None);
        assert!(DwgVersion::R2007.r2004_plus() && !DwgVersion::R2007.r2010_plus());
        assert!(DwgVersion::R14.r13_14() && !DwgVersion::R2000.r13_14());
    }
}

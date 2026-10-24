//! Built-in raster attachment group, used when the seed holds none to clone.
//!
//! The records follow the attachment layout of FORMAT_NOTES ("Raster attachments"). Every
//! byte below is either equal in all complete attachment groups surveyed (2,316 groups in
//! 2D and 3D files) or, for the undecoded display fields that differ between files, the
//! value of one combination observed together: bitonal display with a white foreground and
//! a black background, 200 DPI, the frame on colour 0 with weight 1 and the control records
//! on the default level (64). Ids, times, the range, the placement, the cross ids and the
//! strings are zero here; [`super::raster_template::Template::instantiate`] writes them.

use cadkit_core::{Error, Result};

/// A record as its length and its non-zero byte runs `(offset, hex)`.
pub(super) struct Sparse {
    pub len: usize,
    pub runs: &'static [(usize, &'static str)],
}

impl Sparse {
    pub fn bytes(&self) -> Result<Vec<u8>> {
        let mut out = vec![0; self.len];
        for (offset, hex) in self.runs {
            let run = decode(hex)?;
            let end = offset
                .checked_add(run.len())
                .filter(|end| *end <= self.len)
                .ok_or_else(|| Error::invalid(0, "built-in raster record run out of range"))?;
            out.get_mut(*offset..end)
                .ok_or_else(|| Error::invalid(0, "built-in raster record run out of range"))?
                .copy_from_slice(&run);
        }
        Ok(out)
    }
}

/// Hex digits, spaces ignored.
fn decode(hex: &str) -> Result<Vec<u8>> {
    let digits: Vec<u8> = hex
        .bytes()
        .filter(|b| *b != b' ')
        .map(|b| match b {
            b'0'..=b'9' => Ok(b - b'0'),
            b'a'..=b'f' => Ok(b - b'a' + 10),
            _ => Err(Error::invalid(0, "built-in raster record is not hex")),
        })
        .collect::<Result<_>>()?;
    if !digits.len().is_multiple_of(2) {
        return Err(Error::invalid(
            0,
            "built-in raster record has an odd digit count",
        ));
    }
    Ok(digits
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| p.iter().fold(0, |a, d| a << 4 | d))
        .collect())
}

/// Frame 94 (1024 bytes, no linkages): properties `0x0A00`, weight 1, 200 DPI at `0x118`
/// and `0x120`, and the undecoded display block from `0x128`, unit scales included.
pub(super) const FRAME: Sparse = Sparse {
    len: 0x400,
    runs: &[
        (0x000, "5e00c010 00020000 0002"),
        (0x029, "0a 00000000 0000 01"),
        (0x06c, "05000000 0405"),
        (0x0fc, "05"),
        (0x11e, "6940 0000000000006940 008078ff"),
        (0x136, "f03f 000000000000f03f 00090000 00090000 fe0a"),
        (0x154, "02000000 00ff01"),
        (0x358, "01000000 00000000 11000000 00000000 11"),
        (
            0x376,
            "f03f 000000000000f03f 000000000000f03f 000000000000f03f 000000000000f03f \
             000000000000f03f",
        ),
    ],
};

/// 92 with its dependency linkage `0x271B` (root at `0x84`).
pub(super) const LINK: Sparse = Sparse {
    len: 0x90,
    runs: &[
        (0x000, "5c00c020 48000000 3c000000 40"),
        (0x020, "02000000 01000100 01"),
        (0x078, "0b10d056 1b270100 000301"),
    ],
};

/// The two 93 records, subtypes `0x1000C` and `0x1000D`.
pub(super) const OPTIONS: [Sparse; 2] = [
    Sparse {
        len: 0x68,
        runs: &[(0x000, "5d00c060 34000000 34000000 40"), (0x030, "0c0001")],
    },
    Sparse {
        len: 0x98,
        runs: &[
            (0x000, "5d00c060 4c000000 4c000000 40"),
            (0x030, "0d000100 00000000 11000000 11"),
            (
                0x046,
                "f03f 000000000000f03f 000000000000f03f 000000000000f03f 000000000000f03f \
                 000000000000f03f 00000040",
            ),
        ],
    },
];

/// 90 up to its linkages (the strings and the dependency are appended per attachment).
pub(super) const REFERENCE: Sparse = Sparse {
    len: 0x78,
    runs: &[
        (0x000, "5a00c020 f8000000 3c000000 40"),
        (0x020, "05000000 03000500"),
    ],
};

/// The 90 dependency linkage `0x271B` (root at 12).
pub(super) const REFERENCE_DEPENDENCY: Sparse = Sparse {
    len: 0x18,
    runs: &[(0x000, "0b10d056 1b270100 000301")],
};

/// The five 91 records in stored order: placement `0x20007`, display `0x20008`
/// (foreground `FFFFFF`, background `000000`), `0x10009` and the two `0x1000B` holding
/// the constants "(RM-All)" and "(RM-Defined)".
pub(super) const SETTINGS: [Sparse; 5] = [
    Sparse {
        len: 0xb8,
        runs: &[
            (0x000, "5b00c040 5c000000 5c000000 40"),
            (0x020, "070002"),
            (0x073, "20"),
        ],
    },
    Sparse {
        len: 0x60,
        runs: &[
            (0x000, "5b00c040 30000000 30000000 40"),
            (0x020, "08000200 00000000 00407cff 0000ffff ffffffff"),
            (0x046, "f03f 000000000000f03f"),
        ],
    },
    Sparse {
        len: 0x230,
        runs: &[
            (0x000, "5b00c040 18010000 18010000 40"),
            (0x020, "09000100 00000000 02000000 00ff01"),
        ],
    },
    Sparse {
        len: 0x238,
        runs: &[
            (0x000, "5b00c040 1c010000 1c010000 40"),
            (
                0x020,
                "0b000100 00000000 28005200 4d002d00 41006c00 6c002900",
            ),
            (0x226, "ff"),
        ],
    },
    Sparse {
        len: 0x238,
        runs: &[
            (0x000, "5b00c040 1c010000 1c010000 40"),
            (
                0x020,
                "0b000100 00000000 28005200 4d002d00 44006500 66006900 6e006500 64002900",
            ),
        ],
    },
];

/// Data of the XAttribute 170 sub-records; 3 and 4 (file name and URL) are written per
/// attachment.
pub(super) const XATTRIBUTE_SUBS: [Sparse; 8] = [
    Sparse { len: 4, runs: &[] },
    Sparse {
        len: 0x42,
        runs: &[(
            0x000,
            "02000000 01000000 01000000 ff 28005200 4d002d00 41006c00 6c002900 0000 \
             01000000 01000000 00 28005200 4d002d00 44006500 66006900 6e006500 64002900",
        )],
    },
    Sparse {
        len: 0x28,
        runs: &[(0x000, "01000000 01")],
    },
    Sparse { len: 0, runs: &[] },
    Sparse { len: 0, runs: &[] },
    Sparse { len: 2, runs: &[] },
    Sparse { len: 2, runs: &[] },
    Sparse { len: 4, runs: &[] },
];

/// Flags of the XAttribute 170 record header and its trailer.
pub(super) const XATTRIBUTE_FLAGS: u32 = 8;
pub(super) const XATTRIBUTE_TRAILER: [u8; 4] = [1, 0, 0, 0];
/// Flags (`u16` at 10) of every XAttribute 170 sub-record header.
pub(super) const XATTRIBUTE_SUB_FLAGS: u16 = 0x8000;
/// Offset of the frame id in the 92 dependency linkage.
pub(super) const LINK_ROOT: usize = 0x84;

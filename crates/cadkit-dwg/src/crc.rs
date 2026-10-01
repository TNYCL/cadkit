//! Checksums used by DWG files (ODA spec §2.14, §4.2, §5.4.1).

/// Table of the 16-bit CRC the spec calls "8-bit CRC" (§2.14.1): CRC-16/ARC
/// polynomial 0xA001, reflected.
const CRC8_TABLE: [u16; 256] = build_crc16_table();

// Evaluated at compile time: an out-of-range index would fail the build, not panic.
#[allow(clippy::indexing_slicing)]
const fn build_crc16_table() -> [u16; 256] {
    let mut table = [0u16; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u16;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xA001
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

/// The spec's "8-bit CRC" (a 16-bit table CRC, §2.14.1) over `data`, starting from `seed`.
/// Objects and the header use seed 0xC0C1; the R13–R15 file header uses 0.
pub fn crc8(seed: u16, data: &[u8]) -> u16 {
    let mut dx = seed;
    for &byte in data {
        let al = byte ^ (dx & 0xFF) as u8;
        dx = (dx >> 8) ^ CRC8_TABLE.get(usize::from(al)).copied().unwrap_or(0);
    }
    dx
}

const CRC32_TABLE: [u32; 256] = build_crc32_table();

// Evaluated at compile time: an out-of-range index would fail the build, not panic.
#[allow(clippy::indexing_slicing)]
const fn build_crc32_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

/// The 32-bit CRC of §2.14.2 (standard reflected CRC-32) with an explicit seed.
pub fn crc32(seed: u32, data: &[u8]) -> u32 {
    let mut inverted = !seed;
    for &byte in data {
        inverted = (inverted >> 8)
            ^ CRC32_TABLE
                .get(usize::from((inverted as u8) ^ byte))
                .copied()
                .unwrap_or(0);
    }
    !inverted
}

/// R2004 section page checksum (§4.2): an Adler-32 variant with a seed and
/// 0x15B0-byte chunks.
pub fn page_checksum(seed: u32, data: &[u8]) -> u32 {
    let mut sum1 = seed & 0xFFFF;
    let mut sum2 = seed >> 16;
    for chunk in data.chunks(0x15B0) {
        for &byte in chunk {
            sum1 = sum1.wrapping_add(u32::from(byte));
            sum2 = sum2.wrapping_add(sum1);
        }
        sum1 %= 0xFFF1;
        sum2 %= 0xFFF1;
    }
    (sum2 << 16) | (sum1 & 0xFFFF)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc8_table_matches_spec_excerpt() {
        // First and last rows of the table printed in §2.14.1.
        assert_eq!(
            &CRC8_TABLE[..8],
            &[
                0x0000, 0xC0C1, 0xC181, 0x0140, 0xC301, 0x03C0, 0x0280, 0xC241
            ]
        );
        assert_eq!(
            &CRC8_TABLE[248..],
            &[
                0x8201, 0x42C0, 0x4380, 0x8341, 0x4100, 0x81C1, 0x8081, 0x4040
            ]
        );
    }

    #[test]
    fn crc8_is_crc16_arc() {
        // CRC-16/ARC check value.
        assert_eq!(crc8(0, b"123456789"), 0xBB3D);
        // Accumulation over two calls equals one call.
        assert_eq!(
            crc8(crc8(0xC0C1, b"1234"), b"56789"),
            crc8(0xC0C1, b"123456789")
        );
    }

    #[test]
    fn crc32_table_and_check_value() {
        assert_eq!(
            &CRC32_TABLE[..4],
            &[0x0000_0000, 0x7707_3096, 0xEE0E_612C, 0x9909_51BA]
        );
        assert_eq!(CRC32_TABLE[255], 0x2D02_EF8D);
        assert_eq!(crc32(0, b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn page_checksum_is_adler_like() {
        // With seed 1 this is plain Adler-32: "Wikipedia" -> 0x11E60398.
        assert_eq!(page_checksum(1, b"Wikipedia"), 0x11E6_0398);
        assert_eq!(page_checksum(0, &[]), 0);
    }
}

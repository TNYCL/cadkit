//! Resource accounting against [`Limits`] for one read.

use cadkit_core::{Error, Limits, Result};

/// Running totals of decompressed bytes and decoded objects for one document.
#[derive(Debug, Clone)]
pub(crate) struct Budget {
    pub(crate) limits: Limits,
    decompressed: u64,
    objects: u64,
}

impl Budget {
    pub(crate) fn new(limits: Limits) -> Self {
        Self {
            limits,
            decompressed: 0,
            objects: 0,
        }
    }

    /// Bytes that may still be decompressed or copied out of the container.
    ///
    /// On 32-bit targets (wasm32) a limit above `usize::MAX` saturates; callers that need a
    /// tighter bound pass smaller `Limits`. Every large allocation is bounded by this value:
    /// zlib output (`zlib::inflate` stops at it), compound-file stream copies (checked against
    /// it before reading and charged afterwards), and the elements/records walked from those
    /// buffers. The compound file's own tables are bounded by the input size, which is
    /// checked against `max_input_bytes` before parsing.
    pub(crate) fn remaining_bytes(&self) -> usize {
        usize::try_from(
            self.limits
                .max_decompressed_bytes
                .saturating_sub(self.decompressed),
        )
        .unwrap_or(usize::MAX)
    }

    /// Records `n` decompressed bytes.
    pub(crate) fn charge_bytes(&mut self, n: usize) -> Result<()> {
        self.decompressed = self.decompressed.saturating_add(n as u64);
        if self.decompressed > self.limits.max_decompressed_bytes {
            return Err(Error::LimitExceeded(format!(
                "decompressed data exceeds {} bytes",
                self.limits.max_decompressed_bytes
            )));
        }
        Ok(())
    }

    /// Records `n` decoded objects (elements, records).
    pub(crate) fn charge_objects(&mut self, n: u64) -> Result<()> {
        self.objects = self.objects.saturating_add(n);
        if self.objects > self.limits.max_objects {
            return Err(Error::LimitExceeded(format!(
                "more than {} objects",
                self.limits.max_objects
            )));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limits_are_enforced() {
        let limits = Limits {
            max_decompressed_bytes: 10,
            max_objects: 2,
            ..Limits::default()
        };
        let mut b = Budget::new(limits);
        assert!(b.charge_bytes(10).is_ok());
        assert_eq!(b.remaining_bytes(), 0);
        assert!(b.charge_bytes(1).is_err());
        assert!(b.charge_objects(2).is_ok());
        assert!(b.charge_objects(1).is_err());
    }
}

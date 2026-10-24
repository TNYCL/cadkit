//! Typed access to the native DGN symbology the reader keeps in `props`.
//!
//! The neutral model carries resolved colors and lineweights; the DGN indices behind them
//! live in `props` under [`LEVEL_ID`], [`COLOR_INDEX`], [`WEIGHT`] and [`STYLE`], where the
//! writer looks for them first. [`Symbology`] reads and writes those keys without string
//! literals. The keys stay the storage, so documents read by older versions keep working.

use cadkit_core::{Props, Value};

/// Props key of the DGN level id (element header level, 1-based in V8).
pub const LEVEL_ID: &str = "dgn.level_id";
/// Props key of the DGN color index into the file's 256-entry color table.
pub const COLOR_INDEX: &str = "dgn.color_index";
/// Props key of the DGN line weight index (0..=31).
pub const WEIGHT: &str = "dgn.weight";
/// Props key of the DGN line style number (0..=7 standard, larger for custom styles).
pub const STYLE: &str = "dgn.style";

/// Native DGN symbology of an entity, layer or attribute, as stored in its `props`.
///
/// `None` means the key is absent (or does not hold an integer in `u32` range); the writer
/// then falls back to the neutral color, lineweight and linetype.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub struct Symbology {
    /// [`LEVEL_ID`].
    pub level: Option<u32>,
    /// [`COLOR_INDEX`].
    pub color_index: Option<u32>,
    /// [`WEIGHT`].
    pub weight: Option<u32>,
    /// [`STYLE`].
    pub style: Option<u32>,
}

impl Symbology {
    /// No native symbology.
    pub const fn new() -> Self {
        Self {
            level: None,
            color_index: None,
            weight: None,
            style: None,
        }
    }

    /// Reads the four keys from `props`.
    pub fn from_props(props: &Props) -> Self {
        let get = |key: &str| match props.get(key) {
            Some(Value::Int(n)) => u32::try_from(*n).ok(),
            _ => None,
        };
        Self {
            level: get(LEVEL_ID),
            color_index: get(COLOR_INDEX),
            weight: get(WEIGHT),
            style: get(STYLE),
        }
    }

    /// Writes the four keys into `props`: `Some` stores the value, `None` removes the key.
    pub fn write_props(&self, props: &mut Props) {
        for (key, value) in [
            (LEVEL_ID, self.level),
            (COLOR_INDEX, self.color_index),
            (WEIGHT, self.weight),
            (STYLE, self.style),
        ] {
            match value {
                Some(v) => {
                    props.insert(key.to_owned(), Value::Int(i64::from(v)));
                }
                None => {
                    props.remove(key);
                }
            }
        }
    }

    /// Sets [`Self::level`].
    #[must_use]
    pub const fn with_level(mut self, level: u32) -> Self {
        self.level = Some(level);
        self
    }

    /// Sets [`Self::color_index`].
    #[must_use]
    pub const fn with_color_index(mut self, color_index: u32) -> Self {
        self.color_index = Some(color_index);
        self
    }

    /// Sets [`Self::weight`].
    #[must_use]
    pub const fn with_weight(mut self, weight: u32) -> Self {
        self.weight = Some(weight);
        self
    }

    /// Sets [`Self::style`].
    #[must_use]
    pub const fn with_style(mut self, style: u32) -> Self {
        self.style = Some(style);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_props_and_ignores_foreign_values() {
        let mut props = Props::new();
        props.insert("dxf.other".into(), Value::Int(1));
        props.insert(STYLE.into(), Value::Text("dashed".into()));
        assert_eq!(Symbology::from_props(&props), Symbology::new());
        let s = Symbology::new()
            .with_level(3)
            .with_color_index(5)
            .with_weight(2);
        s.write_props(&mut props);
        assert_eq!(Symbology::from_props(&props), s);
        // `None` removes the key, including the non-integer style above.
        assert!(!props.contains_key(STYLE));
        assert_eq!(props.get(COLOR_INDEX), Some(&Value::Int(5)));
        assert!(props.contains_key("dxf.other"));
        props.insert(WEIGHT.into(), Value::Int(-1));
        assert_eq!(Symbology::from_props(&props).weight, None);
    }
}

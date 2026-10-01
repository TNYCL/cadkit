//! Length-unit helpers: conversion to meters and DWG/DXF `$INSUNITS` codes.

use crate::model::{LengthUnit, Units};

impl LengthUnit {
    /// Size of one unit in meters; `None` for [`LengthUnit::Unitless`] and [`LengthUnit::Custom`].
    pub fn meters(self) -> Option<f64> {
        Some(match self {
            Self::Unitless | Self::Custom => return None,
            Self::Inch => 0.0254,
            Self::Foot => 0.3048,
            Self::Mile => 1609.344,
            Self::Millimeter => 0.001,
            Self::Centimeter => 0.01,
            Self::Meter => 1.0,
            Self::Kilometer => 1000.0,
            Self::Microinch => 2.54e-8,
            Self::Mil => 2.54e-5,
            Self::Yard => 0.9144,
            Self::Angstrom => 1e-10,
            Self::Nanometer => 1e-9,
            Self::Micrometer => 1e-6,
            Self::Decimeter => 0.1,
            Self::Decameter => 10.0,
            Self::Hectometer => 100.0,
            Self::Gigameter => 1e9,
            Self::AstronomicalUnit => 149_597_870_700.0,
            Self::LightYear => 9_460_730_472_580_800.0,
            Self::Parsec => 3.085_677_581_491_367e16,
            Self::UsSurveyFoot => 1200.0 / 3937.0,
        })
    }

    /// Maps a DWG/DXF `$INSUNITS` value to a unit. Unknown codes give [`LengthUnit::Unitless`].
    pub fn from_insunits(code: i32) -> Self {
        match code {
            1 => Self::Inch,
            2 => Self::Foot,
            3 => Self::Mile,
            4 => Self::Millimeter,
            5 => Self::Centimeter,
            6 => Self::Meter,
            7 => Self::Kilometer,
            8 => Self::Microinch,
            9 => Self::Mil,
            10 => Self::Yard,
            11 => Self::Angstrom,
            12 => Self::Nanometer,
            13 => Self::Micrometer,
            14 => Self::Decimeter,
            15 => Self::Decameter,
            16 => Self::Hectometer,
            17 => Self::Gigameter,
            18 => Self::AstronomicalUnit,
            19 => Self::LightYear,
            20 => Self::Parsec,
            21 => Self::UsSurveyFoot,
            _ => Self::Unitless,
        }
    }

    /// The `$INSUNITS` code of this unit (0 for unitless and custom units).
    pub fn to_insunits(self) -> i32 {
        match self {
            Self::Unitless | Self::Custom => 0,
            Self::Inch => 1,
            Self::Foot => 2,
            Self::Mile => 3,
            Self::Millimeter => 4,
            Self::Centimeter => 5,
            Self::Meter => 6,
            Self::Kilometer => 7,
            Self::Microinch => 8,
            Self::Mil => 9,
            Self::Yard => 10,
            Self::Angstrom => 11,
            Self::Nanometer => 12,
            Self::Micrometer => 13,
            Self::Decimeter => 14,
            Self::Decameter => 15,
            Self::Hectometer => 16,
            Self::Gigameter => 17,
            Self::AstronomicalUnit => 18,
            Self::LightYear => 19,
            Self::Parsec => 20,
            Self::UsSurveyFoot => 21,
        }
    }
}

impl Units {
    /// Size of one drawing unit in meters. The explicit `meters_per_unit` field wins when it is
    /// a positive finite number; otherwise the named unit is used. `None` when unknown.
    pub fn meters_per_unit(&self) -> Option<f64> {
        match self.meters_per_unit {
            Some(m) if m.is_finite() && m > 0.0 => Some(m),
            _ => self.unit.meters(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meters() {
        assert_eq!(LengthUnit::Millimeter.meters(), Some(0.001));
        assert_eq!(LengthUnit::Unitless.meters(), None);
        assert_eq!(LengthUnit::Custom.meters(), None);
        assert!((LengthUnit::Foot.meters().unwrap() - 0.3048).abs() < 1e-12);
    }

    #[test]
    fn insunits_roundtrip() {
        for code in 0..=21 {
            assert_eq!(LengthUnit::from_insunits(code).to_insunits(), code);
        }
        assert_eq!(LengthUnit::from_insunits(99), LengthUnit::Unitless);
        assert_eq!(LengthUnit::from_insunits(-1), LengthUnit::Unitless);
        assert_eq!(LengthUnit::Custom.to_insunits(), 0);
    }

    #[test]
    fn explicit_field_preferred() {
        let u = Units {
            unit: LengthUnit::Meter,
            meters_per_unit: Some(0.5),
        };
        assert_eq!(u.meters_per_unit(), Some(0.5));
        let u = Units {
            unit: LengthUnit::Meter,
            meters_per_unit: None,
        };
        assert_eq!(u.meters_per_unit(), Some(1.0));
        let u = Units {
            unit: LengthUnit::Meter,
            meters_per_unit: Some(f64::NAN),
        };
        assert_eq!(u.meters_per_unit(), Some(1.0));
        let u = Units {
            unit: LengthUnit::Custom,
            meters_per_unit: Some(0.01),
        };
        assert_eq!(u.meters_per_unit(), Some(0.01));
        assert_eq!(Units::default().meters_per_unit(), None);
    }
}

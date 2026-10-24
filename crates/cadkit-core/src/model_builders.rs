//! Constructors and `with_*` setters for the `#[non_exhaustive]` model types.
//!
//! The model structs keep their fields public for reading and in-place edits, but code
//! outside this crate cannot build them with struct literals, so new fields can be added
//! in minor releases. These constructors are the stable way to create them.

use crate::geom::Point3;
use crate::model::{
    Attribute, AttributeDisplay, Block, Color, Entity, Format, HAlign, Layer, LengthUnit, Linetype,
    Lineweight, Model, ModelKind, Props, Raw, SourceInfo, TextStyle, Units, VAlign, Value, Warning,
};

/// Generates documented, `#[must_use]` `with_*` setters for public struct fields.
///
/// - `plain`: takes the field type as is.
/// - `into`: takes `impl Into<T>`.
/// - `some`: takes `T` and stores `Some(value)`.
/// - `some_into`: takes `impl Into<T>` and stores `Some(value.into())`.
/// - `prop`: inserts one entry into the `props` map.
macro_rules! setters {
    ($ty:ty { $($kind:ident $name:ident($field:ident: $fty:ty)),* $(,)? }) => {
        impl $ty {
            $(setters!(@one $kind $name $field $fty);)*
        }
    };
    (@one plain $name:ident $field:ident $fty:ty) => {
        #[doc = concat!("Sets [`Self::", stringify!($field), "`].")]
        #[must_use]
        pub fn $name(mut self, value: $fty) -> Self {
            self.$field = value;
            self
        }
    };
    (@one into $name:ident $field:ident $fty:ty) => {
        #[doc = concat!("Sets [`Self::", stringify!($field), "`].")]
        #[must_use]
        pub fn $name(mut self, value: impl Into<$fty>) -> Self {
            self.$field = value.into();
            self
        }
    };
    (@one some $name:ident $field:ident $fty:ty) => {
        #[doc = concat!("Sets [`Self::", stringify!($field), "`] to `Some(value)`.")]
        #[must_use]
        pub fn $name(mut self, value: $fty) -> Self {
            self.$field = Some(value);
            self
        }
    };
    (@one some_into $name:ident $field:ident $fty:ty) => {
        #[doc = concat!("Sets [`Self::", stringify!($field), "`] to `Some(value)`.")]
        #[must_use]
        pub fn $name(mut self, value: impl Into<$fty>) -> Self {
            self.$field = Some(value.into());
            self
        }
    };
    (@one prop $name:ident $field:ident $fty:ty) => {
        #[doc = concat!("Inserts one entry into [`Self::", stringify!($field), "`].")]
        #[must_use]
        pub fn $name(mut self, key: impl Into<String>, value: impl Into<$fty>) -> Self {
            self.$field.insert(key.into(), value.into());
            self
        }
    };
}

impl SourceInfo {
    /// Source information for `format` at the version string written in the file.
    #[must_use]
    pub fn new(format: Format, version: impl Into<String>) -> Self {
        Self {
            format,
            version: version.into(),
            ..Self::default()
        }
    }
}

setters!(SourceInfo {
    some_into with_application(application: String),
    some_into with_codepage(codepage: String),
});

impl Units {
    /// Units named by `unit`, with no explicit meter scale.
    #[must_use]
    pub const fn new(unit: LengthUnit) -> Self {
        Self {
            unit,
            meters_per_unit: None,
        }
    }
}

setters!(Units {
    some with_meters_per_unit(meters_per_unit: f64),
});

impl Layer {
    /// A visible, plottable, unlocked and thawed layer named `name` with by-layer
    /// symbology. `Layer::default()` instead has every flag `false`.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            visible: true,
            plottable: true,
            ..Self::default()
        }
    }
}

setters!(Layer {
    into with_name(name: String),
    some with_id(id: u64),
    plain with_color(color: Color),
    some_into with_linetype(linetype: String),
    plain with_lineweight(lineweight: Lineweight),
    plain with_visible(visible: bool),
    plain with_frozen(frozen: bool),
    plain with_locked(locked: bool),
    plain with_plottable(plottable: bool),
    some_into with_description(description: String),
    plain with_props(props: Props),
    prop with_prop(props: Value),
});

impl Linetype {
    /// A linetype named `name` with the dash `pattern` in drawing units
    /// (positive = dash, negative = gap, zero = dot; empty = continuous).
    #[must_use]
    pub fn new(name: impl Into<String>, pattern: Vec<f64>) -> Self {
        Self {
            name: name.into(),
            pattern,
            ..Self::default()
        }
    }
}

setters!(Linetype {
    some_into with_description(description: String),
    plain with_props(props: Props),
    prop with_prop(props: Value),
});

impl TextStyle {
    /// A variable-height, upright text style named `name` with width factor 1.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            width_factor: 1.0,
            ..Self::default()
        }
    }
}

setters!(TextStyle {
    some_into with_font(font: String),
    plain with_height(height: f64),
    plain with_width_factor(width_factor: f64),
    plain with_oblique(oblique: f64),
    plain with_props(props: Props),
    prop with_prop(props: Value),
});

impl Block {
    /// An empty, local (non-xref) block definition named `name` based at the origin.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }
}

setters!(Block {
    some with_id(id: u64),
    plain with_base_point(base_point: Point3),
    plain with_entities(entities: Vec<Entity>),
    plain with_is_xref(is_xref: bool),
    some_into with_xref_path(xref_path: String),
    some_into with_description(description: String),
    plain with_props(props: Props),
    prop with_prop(props: Value),
});

impl Model {
    /// An empty 2D space named `name` of `kind`.
    #[must_use]
    pub fn new(name: impl Into<String>, kind: ModelKind) -> Self {
        Self {
            name: name.into(),
            kind,
            ..Self::default()
        }
    }
}

setters!(Model {
    some with_id(id: u64),
    plain with_is_3d(is_3d: bool),
    plain with_entities(entities: Vec<Entity>),
    plain with_props(props: Props),
    prop with_prop(props: Value),
});

setters!(Entity {
    some with_id(id: u64),
    plain with_kind(kind: crate::model::EntityKind),
    plain with_attributes(attributes: Vec<Attribute>),
    some with_raw(raw: Raw),
    plain with_props(props: Props),
});

impl Attribute {
    /// A displayed attribute `tag` = `value` with no tag set, position or display data.
    #[must_use]
    pub fn new(tag: impl Into<String>, value: impl Into<Value>) -> Self {
        Self {
            tag: tag.into(),
            value: value.into(),
            ..Self::default()
        }
    }
}

setters!(Attribute {
    some_into with_set(set: String),
    some with_position(position: Point3),
    plain with_invisible(invisible: bool),
    some_into with_layer(layer: String),
    some with_display(display: AttributeDisplay),
    plain with_props(props: Props),
    prop with_prop(props: Value),
});

impl AttributeDisplay {
    /// Unrotated, left-baseline attribute text with the given height and character width,
    /// both in drawing units.
    #[must_use]
    pub fn new(height: f64, width: f64) -> Self {
        Self {
            height,
            width,
            ..Self::default()
        }
    }
}

setters!(AttributeDisplay {
    some_into with_style(style: String),
    plain with_halign(halign: HAlign),
    plain with_valign(valign: VAlign),
    plain with_rotation(rotation: f64),
});

impl Warning {
    /// A warning with a stable machine-readable `code` and a human-readable `message`.
    #[must_use]
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            offset: None,
            object: None,
        }
    }
}

setters!(Warning {
    some with_offset(offset: u64),
    some with_object(object: u64),
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructors_set_documented_defaults() {
        let layer = Layer::new("A")
            .with_color(Color::aci(1))
            .with_prop("dgn.level_id", 7i64);
        assert!(layer.visible && layer.plottable && !layer.frozen && !layer.locked);
        assert_eq!(layer.props.len(), 1);
        assert_eq!(TextStyle::new("S").width_factor, 1.0);
        let warning = Warning::new("x.code", "message").with_offset(4);
        assert_eq!((warning.offset, warning.object), (Some(4), None));
        let units = Units::new(LengthUnit::Custom).with_meters_per_unit(0.3);
        assert_eq!(units.meters_per_unit, Some(0.3));
    }
}

//! Layout and value checks for the `#[repr(C)]` types. `tests/abi.c` asserts the same numbers
//! against `include/cadkit.h`, so a drift on either side fails a build.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use std::mem::{align_of, offset_of, size_of};

use cadkit_capi::*;

#[test]
fn enum_values_are_stable() {
    assert_eq!(size_of::<cadkit_status>(), size_of::<i32>());
    assert_eq!(size_of::<cadkit_format>(), size_of::<i32>());
    let status = [
        (cadkit_status::Ok as i32, 0),
        (cadkit_status::NullPointer as i32, 1),
        (cadkit_status::InvalidArgument as i32, 2),
        (cadkit_status::Io as i32, 3),
        (cadkit_status::Truncated as i32, 4),
        (cadkit_status::InvalidData as i32, 5),
        (cadkit_status::Unsupported as i32, 6),
        (cadkit_status::LimitExceeded as i32, 7),
        (cadkit_status::UnknownFormat as i32, 8),
        (cadkit_status::Panic as i32, 9),
        (cadkit_status::Internal as i32, 10),
        (cadkit_status::InvalidHandle as i32, 11),
    ];
    for (actual, expected) in status {
        assert_eq!(actual, expected);
    }
    let format = [
        (cadkit_format::Unknown as i32, 0),
        (cadkit_format::Dwg as i32, 1),
        (cadkit_format::Dxf as i32, 2),
        (cadkit_format::DgnV7 as i32, 3),
        (cadkit_format::DgnV8 as i32, 4),
    ];
    for (actual, expected) in format {
        assert_eq!(actual, expected);
    }
    let dxf = [
        cadkit_dxf_version::R12 as i32,
        cadkit_dxf_version::R2000 as i32,
        cadkit_dxf_version::R2004 as i32,
        cadkit_dxf_version::R2007 as i32,
        cadkit_dxf_version::R2010 as i32,
        cadkit_dxf_version::R2013 as i32,
        cadkit_dxf_version::R2018 as i32,
    ];
    assert_eq!(dxf, [0, 1, 2, 3, 4, 5, 6]);
}

#[cfg(target_pointer_width = "64")]
#[test]
fn struct_layouts_64_bit() {
    assert_eq!(size_of::<cadkit_options>(), 48);
    assert_eq!(align_of::<cadkit_options>(), 8);
    assert_eq!(offset_of!(cadkit_options, max_input_bytes), 0);
    assert_eq!(offset_of!(cadkit_options, max_decompressed_bytes), 8);
    assert_eq!(offset_of!(cadkit_options, max_objects), 16);
    assert_eq!(offset_of!(cadkit_options, max_depth), 24);
    assert_eq!(offset_of!(cadkit_options, max_string_bytes), 28);
    assert_eq!(offset_of!(cadkit_options, max_vertices), 32);
    assert_eq!(offset_of!(cadkit_options, keep_raw), 36);
    assert_eq!(offset_of!(cadkit_options, fallback_codepage), 40);

    assert_eq!(size_of::<cadkit_svg_options>(), 40);
    assert_eq!(align_of::<cadkit_svg_options>(), 8);
    assert_eq!(offset_of!(cadkit_svg_options, model_index), 0);
    assert_eq!(offset_of!(cadkit_svg_options, width_px), 8);
    assert_eq!(offset_of!(cadkit_svg_options, stroke_px), 16);
    assert_eq!(offset_of!(cadkit_svg_options, background), 24);
    assert_eq!(offset_of!(cadkit_svg_options, text), 32);
    assert_eq!(offset_of!(cadkit_svg_options, expand_blocks), 36);

    assert_eq!(size_of::<cadkit_info>(), 48);
    assert_eq!(align_of::<cadkit_info>(), 8);
    assert_eq!(offset_of!(cadkit_info, format), 0);
    assert_eq!(offset_of!(cadkit_info, model_count), 8);
    assert_eq!(offset_of!(cadkit_info, layer_count), 16);
    assert_eq!(offset_of!(cadkit_info, block_count), 24);
    assert_eq!(offset_of!(cadkit_info, entity_count), 32);
    assert_eq!(offset_of!(cadkit_info, warning_count), 40);
}

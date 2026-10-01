//! Keeps `include/cadkit.h` in sync with the exported symbols.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

#[test]
fn every_exported_function_is_declared_in_the_header() {
    let src = include_str!("../src/lib.rs");
    let header = include_str!("../include/cadkit.h");
    let prefixes = ["pub unsafe extern \"C\" fn ", "pub extern \"C\" fn "];
    let mut found = 0;
    for line in src.lines() {
        let line = line.trim();
        let Some(rest) = prefixes.iter().find_map(|p| line.strip_prefix(p)) else {
            continue;
        };
        let name: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        assert!(
            header.contains(&format!("{name}(")),
            "{name} is exported but missing from cadkit.h"
        );
        found += 1;
    }
    assert!(
        found >= 15,
        "expected at least 15 exported functions, found {found}"
    );
}

#[test]
fn cpp_wrapper_includes_the_c_header() {
    let hpp = include_str!("../include/cadkit.hpp");
    assert!(hpp.contains("#include \"cadkit.h\""));
}

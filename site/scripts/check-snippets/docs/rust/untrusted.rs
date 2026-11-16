use cadkit::{Error, Limits, ReadOptions, SvgOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 64 MiB input, 256 MiB decompressed, 500k objects, depth 32.
    let options = ReadOptions::new().with_limits(Limits::strict());
    let doc = match cadkit::open_with("upload.dxf", &options) {
        Ok(doc) => doc,
        Err(Error::LimitExceeded { .. }) => return Err("rejected: over a limit".into()),
        Err(e) => return Err(format!("rejected: {e}").into()),
    };
    // A complete SVG within 16 MiB, or Error::LimitExceeded: never a truncated drawing.
    let svg_options = SvgOptions::new().with_max_output_bytes(16 << 20).with_images(false);
    let svg = cadkit::to_svg_bounded(&doc, &svg_options, false)?;
    std::fs::write("upload.svg", svg)?;
    Ok(())
}

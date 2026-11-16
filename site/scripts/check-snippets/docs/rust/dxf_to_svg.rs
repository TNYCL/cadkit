use cadkit::SvgOptions;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let doc = cadkit::open("plan.dxf")?;
    let options = SvgOptions::new()
        .with_model(0) // model space; paper layouts follow
        .with_width_px(1600.0) // the height follows the drawing's aspect ratio
        .with_stroke_px(1.0)
        .with_background(None::<String>) // transparent
        .with_text(true);
    // Hidden and frozen layers are left out; cadkit::to_svg_all_layers includes them.
    std::fs::write("plan.svg", cadkit::to_svg(&doc, &options))?;
    Ok(())
}

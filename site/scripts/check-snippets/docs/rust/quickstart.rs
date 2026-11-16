fn main() -> Result<(), Box<dyn std::error::Error>> {
    // DWG, DGN, DXF or CityGML; the format is detected from the bytes.
    let doc = cadkit::open("plan.dxf")?;
    println!("{:?} {} {:?}", doc.source.format, doc.source.version, doc.units.unit);
    for model in &doc.models {
        println!("{}: {} entities", model.name, model.entities.len());
    }
    for warning in &doc.warnings {
        println!("warning: {} {}", warning.code, warning.message);
    }

    let svg = cadkit::to_svg(&doc, &cadkit::SvgOptions::new().with_width_px(1200.0));
    std::fs::write("plan.svg", svg)?;
    Ok(())
}

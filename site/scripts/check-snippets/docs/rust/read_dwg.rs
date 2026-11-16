fn main() -> Result<(), cadkit::Error> {
    let doc = cadkit::open("plan.dwg")?;
    println!("{} {} layers", doc.source.version, doc.layers.len());
    std::fs::write("plan.svg", cadkit::to_svg(&doc, &cadkit::SvgOptions::new()))?;
    Ok(())
}

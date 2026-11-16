fn main() -> Result<(), cadkit::Error> {
    let doc = cadkit::open("plan.dgn")?; // V7 or V8
    println!("{:?} {} levels", doc.source.format, doc.layers.len());
    std::fs::write("plan.svg", cadkit::to_svg(&doc, &cadkit::SvgOptions::new()))?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let doc = cadkit::open("cadkit-sample-plan.dxf")?;
    for layer in &doc.layers {
        println!("{}", layer.name);
    }
    let options = cadkit::SvgOptions::new().with_width_px(1200.0);
    std::fs::write("plan.svg", cadkit::to_svg(&doc, &options))?;
    Ok(())
}

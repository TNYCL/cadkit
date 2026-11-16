fn main() -> Result<(), cadkit::Error> {
    let doc = cadkit::open("plan.dxf")?; // ASCII or binary
    let dxf = cadkit::to_dxf(&doc, cadkit::DxfVersion::R2000)?;
    std::fs::write("plan-r2000.dxf", dxf)?;
    Ok(())
}

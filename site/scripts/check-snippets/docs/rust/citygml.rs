use cadkit::gml::tkgm::{self, Profile};
use cadkit::gml::{self, ExportOptions, ValidationOptions};
use cadkit::{Limits, ReadOptions};

// The CRS your coordinates are in; cadkit never guesses or transforms one.
const CRS: &str = "urn:ogc:def:crs:EPSG::32635";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let doc = cadkit::open("surfaces.dxf")?; // 3D faces or polygons
    let xml = cadkit::to_citygml(&doc, &ExportOptions::new(CRS, 1))?;
    std::fs::write("model.gml", &xml)?;

    let native = gml::read_native(&std::fs::read("model.gml")?, &ReadOptions::default())?;
    let summary = gml::validate(&native, &ValidationOptions::new())?;
    println!("{summary:?}");

    // Local structural checks only; the receiving system remains the acceptance authority.
    let preflight = tkgm::preflight(&native, Profile::CityModelTender, &Limits::default())?;
    println!("acceptance verified: {}", preflight.acceptance_verified); // always false
    for issue in &preflight.issues {
        println!("{:?} {} {:?}", issue.severity, issue.code, issue.field);
    }
    Ok(())
}

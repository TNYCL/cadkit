use cadkit::ReadOptions;
use cadkit::gml::{self, ImportOptions};

fn main() -> Result<(), cadkit::Error> {
    let native = gml::read_native(&std::fs::read("building.gml")?, &ReadOptions::default())?;
    let doc = gml::to_document(&native, &ImportOptions::new(), &ReadOptions::default())?;
    println!("{} entities", doc.entity_count()); // neutral geometry view
    Ok(())
}

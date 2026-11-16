use cadkit::ReadOptions;
use cadkit::dgn::{SeedPrepareOptions, StripSeedOptions, V8Seed, WriteOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. A seed: a DGN V8 file whose tables you may hand on, without its drawing content.
    let template = std::fs::read("template.dgn")?;
    let options = StripSeedOptions::new().with_model_name(Some("Model"));
    let stripped = cadkit::strip_dgn_seed(&template, &options, &ReadOptions::default())?;
    let report = &stripped.report;
    println!("kept: {} named levels, {} tag sets", report.named_levels, report.tag_sets);
    let seed = V8Seed::from_bytes(&stripped.bytes)?;

    // 2. Adapt the drawing to the seed (model, units, 2D to 3D, colours), then write.
    let doc = seed.prepare_document(&cadkit::open("plan.dxf")?, &SeedPrepareOptions::new())?;
    let bytes = cadkit::to_dgn_with_seed(&doc, &seed, &WriteOptions::new())?;
    std::fs::write("converted.dgn", &bytes)?;

    // 3. Check the container and records with cadkit's own structural report.
    let structure = cadkit::dgn::check_structure(&bytes, &ReadOptions::default())?;
    println!("structure warnings: {}", structure.warnings.len());
    Ok(())
}

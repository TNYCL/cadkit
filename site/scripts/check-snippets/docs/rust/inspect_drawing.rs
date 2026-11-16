use std::collections::BTreeMap;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: inspect_drawing <file>")?;
    let header = cadkit::inspect_file(&path)?; // header only: no entity decoding
    println!("{:?} {} {:?}", header.format, header.version, header.units);

    let doc = cadkit::open(&path)?;
    let mut per_layer: BTreeMap<&str, usize> = BTreeMap::new();
    for entity in doc.models.first().map(|m| m.entities.as_slice()).unwrap_or_default() {
        *per_layer.entry(entity.layer.as_deref().unwrap_or("0")).or_default() += 1;
    }
    for layer in &doc.layers {
        let state = if layer.visible { "on" } else { "off" };
        let count = per_layer.get(layer.name.as_str()).copied().unwrap_or(0);
        println!("{:<24} {state:<4} frozen={} {count} entities", layer.name, layer.frozen);
    }
    for warning in doc.warnings.iter().take(20) {
        println!("warning: {} {}", warning.code, warning.message);
    }
    Ok(())
}

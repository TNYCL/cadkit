//! Generates synthetic DGN V8 and CityGML outputs from a public seed for independent acceptance checks.

use cadkit::{Document, Entity, EntityKind, Model, Point3, ReadOptions, Value};
use std::{io, path::PathBuf};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args_os().skip(1);
    let seed_path = args
        .next()
        .ok_or_else(|| io::Error::other("usage: interchange SEED OUTPUT_DIRECTORY"))?;
    let directory = PathBuf::from(
        args.next()
            .ok_or_else(|| io::Error::other("output directory is required"))?,
    );
    let seed = std::fs::read(seed_path)?;
    let seed_doc = cadkit::read(&seed)?;
    let model = seed_doc
        .models
        .first()
        .ok_or_else(|| io::Error::other("seed has no model"))?;
    let ring = vec![
        Point3::xy(0., 0.),
        Point3::xy(10., 0.),
        Point3::xy(10., 10.),
        Point3::xy(0., 10.),
        Point3::xy(0., 0.),
    ];
    let mut outline = Entity::new(EntityKind::Polygon {
        exterior: ring,
        interiors: vec![vec![
            Point3::xy(2., 2.),
            Point3::xy(2., 3.),
            Point3::xy(3., 3.),
            Point3::xy(3., 2.),
            Point3::xy(2., 2.),
        ]],
    });
    outline.layer = Some("Synthetic surfaces".into());
    let mut doc = {
        let mut doc = Document::default();
        doc.units = seed_doc.units;
        doc
    };
    doc.models.push(
        Model::default()
            .with_is_3d(model.is_3d)
            .with_entities(vec![outline]),
    );
    let options = {
        let mut options = cadkit::dgn::WriteOptions::default().with_clear_seed_model(true);
        options.codepage = Some("windows-1254".into());
        options
    };
    std::fs::create_dir_all(&directory)?;
    std::fs::write(
        directory.join("01-repacked.dgn"),
        cadkit::dgn::repack_v8(&seed, &ReadOptions::default())?,
    )?;
    std::fs::write(
        directory.join("02-new-geometry.dgn"),
        cadkit::to_dgn(&doc, &seed, &options)?,
    )?;
    let gml = cadkit::to_citygml(
        &doc,
        &cadkit::gml::ExportOptions::new("https://example.invalid/crs/synthetic-local", 1)
            .with_limits(cadkit::Limits::default()),
    )?;
    std::fs::write(directory.join("synthetic.gml"), gml)?;
    if let Some(entity) = doc.models.first_mut().and_then(|m| m.entities.first_mut()) {
        entity.attributes.push(
            cadkit::Attribute::new("Açıklama", Value::Text("Sentetik ölçüm".into()))
                .with_set("Örnek")
                .with_invisible(true),
        );
        if let EntityKind::Polygon {
            exterior,
            interiors,
        } = &mut entity.kind
        {
            for p in exterior.iter_mut().chain(interiors.iter_mut().flatten()) {
                p.x += 5.;
            }
        }
    }
    std::fs::write(
        directory.join("03-moved-and-tagged.dgn"),
        cadkit::to_dgn(&doc, &seed, &options)?,
    )?;
    Ok(())
}

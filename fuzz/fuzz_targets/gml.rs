//! Sınırlı kaynaklarla rastgele girdi ve yeniden yazma denemeleri.
#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    let mut options = cadkit_core::ReadOptions::default();
    options.limits.max_input_bytes = 1 << 20;
    options.limits.max_decompressed_bytes = 4 << 20;
    options.limits.max_objects = 10_000;
    options.limits.max_vertices = 10_000;
    options.limits.max_total_vertices = 20_000;
    options.limits.max_depth = 32;
    if let Ok(doc) = cadkit_gml::read_native(data, &options) {
        let _ = cadkit_gml::tkgm::preflight(
            &doc,
            cadkit_gml::tkgm::Profile::CityModelTender,
            &options.limits,
        );
        if let Ok(neutral) =
            cadkit_gml::to_document(&doc, &cadkit_gml::ImportOptions::default(), &options)
        {
            let export = cadkit_gml::ExportOptions {
                srs_name: "urn:cadkit:fuzz".into(),
                lod: 2,
                limits: options.limits,
            };
            let _ = cadkit_gml::write_document_with_report(
                &neutral,
                &export,
                cadkit_gml::UnsupportedGeometry::MetadataOnly,
            );
        }
        let validation = cadkit_gml::ValidationOptions {
            limits: options.limits,
            max_geometry_work: 100_000,
            ..Default::default()
        };
        if let Ok(bytes) = cadkit_gml::write(&doc, &validation) {
            let _ = cadkit_gml::read_native(&bytes, &options);
        }
    }
});

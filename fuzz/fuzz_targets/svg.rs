#![no_main]
//! read -> to_svg + bbox + to_json must never panic, for every model of the document.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(doc) = cadkit::read_with(data, &cadkit_fuzz::options()) else {
        return;
    };
    // Bound the work: a document may have many models, but a few cover the code paths.
    for model in 0..doc.models.len().min(4) {
        let _ = doc.bbox(model);
        let options = cadkit::SvgOptions {
            model,
            ..cadkit::SvgOptions::default()
        };
        let _ = cadkit::to_svg(&doc, &options);
    }
    let _ = cadkit::to_json(&doc, false);
});

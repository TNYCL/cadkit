//! Unchanged attachment graphs must retain opaque bytes and element identities.

use super::*;

fn raster_seed() -> Vec<u8> {
    let mut frame = header(94, 0x400, 100, 0, 0., &line(), false, 0).unwrap();
    let matrix = [
        1., 0., 0., 1000., 0., 1., 0., 2000., 0., 0., 1., 0., 0., 0., 0., 1.,
    ];
    for (i, n) in matrix.into_iter().enumerate() {
        f64_at(&mut frame, 0x78 + i * 8, n).unwrap();
    }
    f64_at(&mut frame, 0x108, 11000.).unwrap();
    f64_at(&mut frame, 0x110, 22000.).unwrap();
    range(&mut frame, &[[1000., 2000., 0.], [11000., 22000., 0.]]).unwrap();
    put(&mut frame, 0x180, b"opaque synthetic frame settings").unwrap();
    let frame = finish(frame, &[]).unwrap();

    let mut reference = prefix(90, 0x78, 200, 0, 0.).unwrap();
    u64_at(&mut reference, 0x38, 201).unwrap();
    let mut links = string_link(3, "synthetic.png").unwrap();
    links.extend(dependency(0x271b, 100).unwrap());
    let reference = finish(reference, &links).unwrap();
    let mut link = prefix(92, 0x78, 201, 0, 0.).unwrap();
    u64_at(&mut link, 0x30, 200).unwrap();
    let link = finish(link, &dependency(0x271b, 100).unwrap()).unwrap();
    let mut setting = prefix(91, 0x60, 202, 0, 0.).unwrap();
    put(&mut setting, 0x28, b"opaque synthetic control settings").unwrap();
    let setting = finish(setting, &[]).unwrap();

    let mut aux = vec![0; 28];
    u32_at(&mut aux, 0, 0xa11b).unwrap();
    u32_at(&mut aux, 4, 4).unwrap();
    u32_at(&mut aux, 8, 0xaa).unwrap();
    u64_at(&mut aux, 16, 100).unwrap();
    aux.extend([1, 2, 3, 4]);
    let mut aux_page = vec![0; 16];
    u32_at(&mut aux_page, 0, 1).unwrap();
    u32_at(&mut aux_page, 4, 3).unwrap();
    aux_page.extend(compressed(&aux).unwrap());

    let mut file = ::cfb::CompoundFile::open(Cursor::new(seed(false))).unwrap();
    for storage in ["Dgn^C", "Dgn^GA"] {
        file.create_storage_all(format!("Dgn-Md/#000000/{storage}"))
            .unwrap();
    }
    for (path, data) in [
        ("Dgn-Md/#000000/Dgn^G/$1", page(&[frame], 3, 1).unwrap()),
        (
            "Dgn-Md/#000000/Dgn^C/$1",
            page(&[reference, link, setting], 3, 1).unwrap(),
        ),
        ("Dgn-Md/#000000/Dgn^GA/$1", aux_page),
    ] {
        file.create_stream(path).unwrap().write_all(&data).unwrap();
    }
    file.flush().unwrap();
    file.into_inner().into_inner()
}

#[test]
fn raster_controls_frames_and_auxiliary_survive_while_other_geometry_changes() {
    let seed = raster_seed();
    let options = WriteOptions {
        clear_seed_model: true,
        ..Default::default()
    };
    let read = ReadOptions::default();
    let mut source = crate::read(&seed, &read).unwrap();
    assert!(matches!(
        source.models[0].entities[0].kind,
        EntityKind::Image { path: Some(_), .. }
    ));
    source.models[0].entities.push(line());
    let output = write_v8(&source, &seed, &options).unwrap();
    let reread = crate::read(&output, &read).unwrap();
    assert_eq!(
        source.models[0].entities[0].kind,
        reread.models[0].entities[0].kind
    );
    assert_eq!(reread.models[0].entities.len(), 2);
    let before = v8::read(&seed, &read).unwrap();
    let after = v8::read(&output, &read).unwrap();
    assert_eq!(
        before.models[0].graphic_pages[0].elements[0].bytes,
        after.models[0].graphic_pages[0].elements[0].bytes
    );
    assert_eq!(
        before.models[0].control_pages,
        after.models[0].control_pages
    );
    assert_eq!(
        before.models[0].graphic_aux[0].records,
        after.models[0].graphic_aux[0].records
    );
    assert_ne!(
        reread.models[0].entities[0].id,
        reread.models[0].entities[1].id
    );
    assert_eq!(
        container::stream(
            &seed,
            "Dgn-Md/#000000/Dgn^C/$1",
            read.limits.max_input_bytes
        )
        .unwrap(),
        container::stream(
            &output,
            "Dgn-Md/#000000/Dgn^C/$1",
            read.limits.max_input_bytes
        )
        .unwrap()
    );
}

#[test]
fn changed_new_duplicate_or_disabled_raster_preservation_is_rejected() {
    let seed = raster_seed();
    let source = crate::read(&seed, &ReadOptions::default()).unwrap();
    let options = WriteOptions {
        clear_seed_model: true,
        ..Default::default()
    };
    for change in 0..4 {
        let mut doc = source.clone();
        let e = &mut doc.models[0].entities[0];
        match change {
            0 => {
                if let EntityKind::Image { position, .. } = &mut e.kind {
                    position.x += 1.;
                }
            }
            1 => e.id = None,
            2 => e.visible = !e.visible,
            _ => e.props.clear(),
        }
        assert!(write_v8(&doc, &seed, &options).is_err());
    }
    let mut duplicate = source.clone();
    duplicate.models[0]
        .entities
        .push(source.models[0].entities[0].clone());
    assert!(write_v8(&duplicate, &seed, &options).is_err());
    assert!(
        write_v8(
            &source,
            &seed,
            &WriteOptions {
                preserve_seed_rasters: false,
                ..options
            }
        )
        .is_err()
    );
}

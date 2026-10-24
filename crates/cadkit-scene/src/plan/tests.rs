use super::*;
use cadkit_geom::{Arc as ArcCurve, Circle, LineSegment, Polyline};

fn instances(curve: Curve) -> Vec<[u32; 9]> {
    let mut writer = Writer {
        bytes: Vec::new(),
        origin: Point2::new(16., 16.),
        extent: 0.,
    };
    writer.curve(&curve, [1, 2, 3, 4], 1.5);
    words(&writer.bytes)
}

fn words(bytes: &[u8]) -> Vec<[u32; 9]> {
    bytes
        .chunks(INSTANCE_BYTES)
        .map(|chunk| {
            let mut words = [0; 9];
            for (word, bytes) in words.iter_mut().zip(chunk.chunks(4)) {
                *word = u32::from_le_bytes(bytes.try_into().unwrap());
            }
            words
        })
        .collect()
}

fn line(x0: f64, y0: f64, x1: f64, y1: f64) -> Curve {
    Curve::Line(LineSegment {
        start: Point2::new(x0, y0),
        end: Point2::new(x1, y1),
    })
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Curve {
    Curve::Polyline(Polyline {
        points: vec![
            Point2::new(x, y),
            Point2::new(x + w, y),
            Point2::new(x + w, y + h),
            Point2::new(x, y + h),
        ],
        closed: true,
    })
}

fn item(group: u32, curves: Vec<Curve>) -> PlanItem {
    PlanItem::new(GroupId(group), curves, [10, 20, 30, 255], 1.)
}

fn view(center: Point2, zoom: f64) -> PlanView {
    PlanView::new(center, zoom, [800., 600.], 800.).with_pixels_per_point(2.)
}

const ALL: fn(GroupId) -> bool = |_| true;

/// Golden bytes from the encoder this crate was ported from, for the curves in
/// `encoding_matches_the_golden_bytes`. Instance layout changes must be deliberate.
const GOLDEN: &str = include_str!("golden.hex");

#[test]
fn encoding_matches_the_golden_bytes() {
    let mut writer = Writer {
        bytes: Vec::new(),
        origin: Point2::new(16., 16.),
        extent: 0.,
    };
    let color = premultiply([200, 100, 50, 128]);
    let curves = [
        rect(10., 10., 4., 2.),
        Curve::Polyline(Polyline {
            points: vec![
                Point2::new(0., 0.),
                Point2::new(3., 0.),
                Point2::new(3., 3.),
            ],
            closed: false,
        }),
        Curve::Circle(Circle {
            center: Point2::new(20., 18.),
            radius: 2.,
        }),
        Curve::Arc(ArcCurve {
            center: Point2::new(16., 16.),
            radius: 1.,
            start_angle: 1.,
            sweep_angle: -2.5,
        }),
        line(-5.25, 7.5, 40.125, -3.0),
    ];
    for curve in &curves {
        writer.curve(curve, color, 1.5);
    }
    let hex: String = writer.bytes.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, GOLDEN.trim());
    assert_eq!(writer.extent, 24.125);
}

#[test]
fn polyline_joints_share_bisector_and_open_ends_are_butt() {
    let square = instances(rect(10., 10., 4., 2.));
    assert_eq!(square.len(), 4);
    for (index, segment) in square.iter().enumerate() {
        let next = square[(index + 1) % 4];
        assert_eq!(segment[8], START_JOINT | END_JOINT);
        // The end corner is the next segment's start point with the same bisector.
        assert_eq!([segment[2], segment[3]], [next[0], next[1]]);
        assert_eq!(segment[5], next[4]);
    }
    let open = instances(Curve::Polyline(Polyline {
        points: vec![
            Point2::new(0., 0.),
            Point2::new(3., 0.),
            Point2::new(3., 3.),
        ],
        closed: false,
    }));
    assert_eq!(open.len(), 2);
    assert_eq!(open[0][8], END_JOINT);
    assert_eq!(open[1][8], START_JOINT);
    assert_eq!(open[0][5], open[1][4]);
    let local = f32::from_bits;
    assert_eq!((local(open[0][0]), local(open[0][1])), (-16., -16.));
}

#[test]
fn arcs_split_into_quarter_pieces_that_share_boundaries() {
    let circle = instances(Curve::Circle(Circle {
        center: Point2::new(20., 18.),
        radius: 2.,
    }));
    assert_eq!(circle.len(), 4);
    for (index, piece) in circle.iter().enumerate() {
        assert_eq!(piece[8], KIND_ARC | START_JOINT | END_JOINT);
        assert_eq!(piece[5], circle[(index + 1) % 4][4]);
    }
    let arc = instances(Curve::Arc(ArcCurve {
        center: Point2::new(16., 16.),
        radius: 1.,
        start_angle: 1.,
        sweep_angle: -2.5,
    }));
    assert_eq!(arc.len(), 2);
    // A clockwise arc is encoded counter-clockwise; its outer ends have no joint.
    assert_eq!(f32::from_bits(arc[0][4]), -1.5);
    assert_eq!(f32::from_bits(arc[1][5]), 1.);
    assert_eq!(arc[0][8], KIND_ARC | END_JOINT);
    assert_eq!(arc[1][8], KIND_ARC | START_JOINT);
    assert_eq!(arc[0][5], arc[1][4]);
}

/// Observable state: per chunk the members and encoded bytes, plus the unbounded set.
type ChunkSummary = (ChunkKey, Vec<ObjectId>, Option<Bounds2>, Vec<u8>);

fn summary(plan: &mut Plan2d) -> Vec<ChunkSummary> {
    let wide = PlanView::new(Point2::new(0., 0.), 1e-6, [800., 600.], 800.);
    plan.prepare(&wide, ALL);
    plan.chunks
        .iter()
        .map(|(key, chunk)| {
            let bytes = chunk
                .instances
                .as_ref()
                .map(|i| i.bytes.to_vec())
                .unwrap_or_default();
            (
                *key,
                chunk.ids.iter().copied().collect(),
                chunk.bounds,
                bytes,
            )
        })
        .collect()
}

fn scene(step: usize) -> Vec<(ObjectId, PlanItem)> {
    let mut items = Vec::new();
    for i in 0..40u64 {
        let x = (i % 8) as f64 * 9. - 30. + step as f64 * 0.37 * (i % 3) as f64;
        let y = (i / 8) as f64 * 11. - 20.;
        let curves = match (i + step as u64) % 4 {
            0 => vec![line(x, y, x + 5., y + 1.)],
            1 => vec![rect(x, y, 3., 2.)],
            2 => vec![Curve::Circle(Circle {
                center: Point2::new(x, y),
                radius: 1.5,
            })],
            _ => vec![],
        };
        if !(i + step as u64).is_multiple_of(7) {
            items.push((ObjectId(i), item((i % 2) as u32, curves)));
        }
    }
    items
}

#[test]
fn incremental_updates_match_a_fresh_build() {
    let mut incremental = Plan2d::new(Plan2dOptions::new().with_chunk_size(16.));
    for step in 0..6 {
        let items = scene(step);
        let present: BTreeSet<ObjectId> = items.iter().map(|(id, _)| *id).collect();
        let stale: Vec<ObjectId> = incremental
            .entries
            .keys()
            .filter(|id| !present.contains(id))
            .copied()
            .collect();
        for id in stale {
            assert!(incremental.remove(id));
        }
        for (id, item) in items.iter().cloned() {
            incremental.upsert(id, item);
        }
        let mut fresh = Plan2d::new(Plan2dOptions::new().with_chunk_size(16.));
        for (id, item) in items {
            fresh.upsert(id, item);
        }
        assert_eq!(
            summary(&mut incremental),
            summary(&mut fresh),
            "step {step}"
        );
        assert_eq!(incremental.always, fresh.always);
        let area = Bounds2 {
            min: Point2::new(-12., -9.),
            max: Point2::new(4., 7.),
        };
        assert_eq!(incremental.near(area, ALL), fresh.near(area, ALL));
    }
}

#[test]
fn near_matches_a_full_scan() {
    let mut plan = Plan2d::new(Plan2dOptions::new().with_chunk_size(8.));
    for (id, item) in scene(2) {
        plan.upsert(id, item);
    }
    for (x, y, r) in [
        (0., 0., 3.),
        (-25., 10., 6.),
        (100., 100., 1.),
        (5., -15., 40.),
    ] {
        let area = Bounds2 {
            min: Point2::new(x - r, y - r),
            max: Point2::new(x + r, y + r),
        };
        let expected: BTreeSet<ObjectId> = plan
            .entries
            .iter()
            .filter(|(_, entry)| {
                entry.item.group == GroupId(1)
                    && entry.item.bounds.is_none_or(|b| {
                        b.max.x >= area.min.x
                            && b.min.x <= area.max.x
                            && b.max.y >= area.min.y
                            && b.min.y <= area.max.y
                    })
            })
            .map(|(id, _)| *id)
            .collect();
        assert_eq!(plan.near(area, |g| g == GroupId(1)), expected);
    }
}

#[test]
fn frame_culls_chunks_hides_overlay_and_falls_back_beyond_precision_budget() {
    let mut plan = Plan2d::new(Plan2dOptions::new());
    plan.upsert(ObjectId(1), item(0, vec![rect(0., 0., 6., 5.)]));
    plan.upsert(ObjectId(2), item(0, vec![line(1., 1., 2., 1.)]));
    plan.upsert(
        ObjectId(3),
        item(0, vec![]).with_label(true).with_bounds(Some(Bounds2 {
            min: Point2::new(2., 2.),
            max: Point2::new(3., 3.),
        })),
    );
    plan.upsert(ObjectId(4), item(1, vec![line(1., 1., 4., 4.)]));
    plan.upsert(ObjectId(5), item(0, vec![line(500., 500., 501., 500.)]));
    plan.upsert(
        ObjectId(6),
        item(0, vec![line(2., 1., 3., 1.)]).with_force_cpu(true),
    );
    let only_0 = |group: GroupId| group == GroupId(0);
    let camera = view(Point2::new(3., 2.5), 100.);
    plan.prepare(&camera, only_0);
    let overlay = BTreeSet::from([ObjectId(1)]);
    let frame = plan.frame(&camera, only_0, &overlay, true);
    assert_eq!(frame.draws.len(), 1);
    assert!(frame.cpu.contains(&ObjectId(1)));
    assert!(!frame.cpu.contains(&ObjectId(4)) && !frame.cpu.contains(&ObjectId(5)));
    assert!(frame.cpu.contains(&ObjectId(6)));
    assert_eq!(frame.labels, BTreeSet::from([ObjectId(3)]));
    // The overlay object's range is cut out of the draw.
    let key = plan.entries[&ObjectId(1)].chunk.unwrap();
    let hidden = plan.chunks[&key].instances.as_ref().unwrap().ranges[&ObjectId(1)].clone();
    let draw = &frame.draws[0];
    assert_eq!(draw.slot, plan.chunks[&key].slot);
    assert!(
        draw.ranges
            .iter()
            .all(|range| range.end <= hidden.start || range.start >= hidden.end)
    );
    // Without the GPU the same objects go to the CPU path.
    let cpu_only = plan.frame(&camera, only_0, &BTreeSet::new(), false);
    assert!(cpu_only.draws.is_empty());
    assert!(cpu_only.cpu.contains(&ObjectId(2)));
    assert!(cpu_only.labels.is_empty());
    // At extreme zoom the f32 chunk coordinates exceed the budget: CPU path.
    let close = view(Point2::new(3., 2.5), 1e7);
    plan.prepare(&close, only_0);
    let fallback = plan.frame(&close, only_0, &BTreeSet::new(), true);
    assert!(fallback.draws.is_empty());
    assert!(fallback.cpu.contains(&ObjectId(1)));
}

#[test]
fn removing_an_object_reencodes_its_chunk() {
    let mut plan = Plan2d::new(Plan2dOptions::new());
    plan.upsert(ObjectId(1), item(0, vec![line(0., 0., 1., 0.)]));
    plan.upsert(ObjectId(2), item(0, vec![line(5., 5., 30., 30.)]));
    let wide = view(Point2::new(10., 10.), 10.);
    plan.prepare(&wide, ALL);
    let before = plan.frame(&wide, ALL, &BTreeSet::new(), true).draws;
    assert_eq!(before.len(), 1);
    assert!(plan.remove(ObjectId(2)));
    assert!(!plan.remove(ObjectId(2)));
    plan.prepare(&wide, ALL);
    let after = plan.frame(&wide, ALL, &BTreeSet::new(), true).draws;
    assert_ne!(after[0].generation, before[0].generation);
    assert_eq!(after[0].slot, before[0].slot);
    assert_eq!(after[0].bytes.len(), INSTANCE_BYTES);
    let key = plan.entries[&ObjectId(1)].chunk.unwrap();
    assert_eq!(
        plan.chunks[&key].bounds,
        Some(Bounds2 {
            min: Point2::new(0., 0.),
            max: Point2::new(1., 0.)
        })
    );
    assert!(plan.remove(ObjectId(1)));
    assert!(plan.chunks.is_empty() && plan.is_empty());
}

#[test]
fn hostile_input_neither_panics_nor_reaches_the_gpu() {
    let mut plan = Plan2d::new(Plan2dOptions::new());
    let nan = f64::NAN;
    let inf = f64::INFINITY;
    let curves = [
        line(nan, 0., 1., 1.),
        line(0., 0., inf, 1.),
        line(1e300, -1e300, 1e300, 1e300),
        Curve::Circle(Circle {
            center: Point2::new(0., 0.),
            radius: 0.,
        }),
        Curve::Arc(ArcCurve {
            center: Point2::new(0., 0.),
            radius: 1.,
            start_angle: 0.,
            sweep_angle: inf,
        }),
        Curve::Polyline(Polyline {
            points: vec![Point2::new(1., 1.), Point2::new(1., 1.)],
            closed: false,
        }),
        Curve::Polyline(Polyline {
            points: vec![],
            closed: true,
        }),
    ];
    for (index, curve) in curves.into_iter().enumerate() {
        let id = ObjectId(index as u64);
        plan.upsert(id, PlanItem::new(GroupId(0), vec![curve], [0; 4], 0.));
        plan.upsert(
            ObjectId(100 + index as u64),
            item(0, vec![line(0., 0., 1., 1.)]).with_bounds(Some(Bounds2 {
                min: Point2::new(nan, -inf),
                max: Point2::new(inf, nan),
            })),
        );
    }
    for camera in [
        view(Point2::new(0., 0.), 1.),
        view(Point2::new(nan, 0.), 1.),
        view(Point2::new(1e300, 1e300), 1e300),
        view(Point2::new(0., 0.), 0.),
        PlanView::new(Point2::new(0., 0.), 1., [0., -1.], f64::INFINITY),
    ] {
        plan.prepare(&camera, ALL);
        let frame = plan.frame(&camera, ALL, &BTreeSet::new(), true);
        for draw in &frame.draws {
            // Only the valid unit lines are on the GPU.
            assert!(words(&draw.bytes).iter().all(|w| w[8] & KIND_ARC == 0));
        }
        let _ = plan.near(
            Bounds2 {
                min: Point2::new(-inf, nan),
                max: Point2::new(inf, inf),
            },
            ALL,
        );
    }
}

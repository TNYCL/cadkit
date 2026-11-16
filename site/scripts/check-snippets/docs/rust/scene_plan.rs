use std::collections::BTreeSet;

use cadkit_geom::Point2;
use cadkit_scene::document::{DocumentPlanOptions, plan_document};
use cadkit_scene::{Plan2d, Plan2dOptions, PlanView};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let doc = cadkit::open("plan.dxf")?;

    // Model 0 to plan items: blocks expanded, XY projection, one group per layer.
    let planned = plan_document(&doc, &DocumentPlanOptions::new());
    let report = &planned.report;
    println!("drawn {}, flattened {}, skipped {:?}", report.drawn, report.flattened, report.skipped);

    let mut plan = Plan2d::new(Plan2dOptions::new());
    for (id, item) in planned.items {
        plan.upsert(id, item);
    }

    // A 1280 x 800 canvas centred on (6, 4.5) at 80 logical pixels per drawing unit.
    let view = PlanView::new(Point2::new(6.0, 4.5), 80.0, [1280.0, 800.0], 1280.0);
    plan.prepare(&view, |_| true);
    let frame = plan.frame(&view, |_| true, &BTreeSet::new(), true);
    println!("{} chunk draws for the GPU, {} objects for the CPU path", frame.draws.len(), frame.cpu.len());
    Ok(())
}

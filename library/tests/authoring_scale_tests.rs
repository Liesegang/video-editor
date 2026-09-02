use std::time::Instant;

use library::core::render_plan::RenderPlanCompiler;
use library::model::authoring::{AuthoringProject, AuthoringSession, SourceRef, TimelineInterval};

#[test]
fn timeline_item_scale_does_not_create_user_or_runtime_nodes() {
    for item_count in [100_usize, 1_000, 10_000] {
        let mut session = AuthoringSession::new(
            AuthoringProject::new("Scale", 1920, 1080, 30.0, 60.0).expect("project"),
        )
        .expect("session");
        let project = session.project();
        let timeline_id = project.root_timeline_id;
        let track_id = project.timelines[&timeline_id].track_order[0];
        for index in 0..item_count {
            session
                .add_item(
                    track_id,
                    format!("Caption {index}"),
                    SourceRef::Text {
                        text: format!("Line {index}"),
                    },
                    TimelineInterval::new(index as f64 * 0.01, 1.0).expect("interval"),
                    index as i64,
                )
                .expect("Timeline item");
        }
        let project = session.into_project();
        let started = Instant::now();
        let plan = RenderPlanCompiler::compile(&project).expect("RenderPlan");
        let elapsed = started.elapsed();
        assert_eq!(plan.timelines[&timeline_id].schedule.len(), item_count);
        assert!(plan.module_definitions.is_empty());
        assert!(plan.module_invocations.is_empty());
        eprintln!(
            "RenderPlan baseline: {item_count} TimelineItems compiled in {:.3} ms",
            elapsed.as_secs_f64() * 1_000.0
        );
    }
}

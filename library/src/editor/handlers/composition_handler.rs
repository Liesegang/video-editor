use crate::error::LibraryError;
use crate::model::project::{Composition, Project};
use crate::model::{NodeContent, ReferenceContent};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

pub struct CompositionHandler;

impl CompositionHandler {
    pub fn update_composition(
        project: &Arc<RwLock<Project>>,
        id: Uuid,
        name: &str,
        width: u32,
        height: u32,
        fps: f64,
        duration: f64,
    ) -> Result<(), LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        let comp =
            proj.compositions
                .iter_mut()
                .find(|c| c.id == id)
                .ok_or(LibraryError::Project(format!(
                    "Composition not found: {}",
                    id
                )))?;

        let new_frame_count =
            validate_composition_parameters(width.into(), height.into(), fps, duration)?;

        if comp.fps.to_bits() != fps.to_bits() || comp.duration.to_bits() != duration.to_bits() {
            update_work_area_for_timing_change(comp, fps, new_frame_count);
        }

        comp.name = name.to_string();
        comp.width = width as u64;
        comp.height = height as u64;
        comp.fps = fps;
        comp.duration = duration;

        Ok(())
    }

    pub fn add_composition(
        project: &Arc<RwLock<Project>>,
        name: &str,
        width: u64,
        height: u64,
        fps: f64,
        duration: f64,
    ) -> Result<Uuid, LibraryError> {
        validate_composition_parameters(width, height, fps, duration)?;

        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;

        // Composition::new returns (Composition, Track)
        let (composition, root_track) = Composition::new(name, width, height, fps, duration);
        let id = composition.id;

        let mut candidate = proj.clone();
        candidate
            .add_track(root_track)
            .and_then(|()| candidate.add_composition(composition))
            .map_err(|error| LibraryError::Project(error.to_string()))?;
        *proj = candidate;

        Ok(id)
    }

    pub fn remove_composition(
        project: &Arc<RwLock<Project>>,
        id: Uuid,
    ) -> Result<Option<Composition>, LibraryError> {
        let mut proj = project
            .write()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        Ok(proj.remove_composition(id))
    }

    pub fn get_composition(
        project: &Arc<RwLock<Project>>,
        id: Uuid,
    ) -> Result<Composition, LibraryError> {
        let proj = project
            .read()
            .map_err(|_| LibraryError::Runtime("Lock Poisoned".to_string()))?;
        proj.compositions
            .iter()
            .find(|c| c.id == id)
            .cloned()
            .ok_or(LibraryError::Project(format!(
                "Composition not found: {}",
                id
            )))
    }

    pub fn is_composition_used(project: &Arc<RwLock<Project>>, comp_id: Uuid) -> bool {
        if let Ok(proj) = project.read() {
            for node in proj.nodes.values() {
                if matches!(
                    node.content(),
                    NodeContent::Reference(ReferenceContent { target_id, .. })
                        if *target_id == comp_id
                ) {
                    return true;
                }
            }
        }
        false
    }
}

fn validate_composition_parameters(
    width: u64,
    height: u64,
    fps: f64,
    duration: f64,
) -> Result<u64, LibraryError> {
    Composition::validate_settings(width, height, fps, duration)
        .map_err(|error| LibraryError::Validation(format!("Invalid Composition: {error}")))
}

fn update_work_area_for_timing_change(comp: &mut Composition, fps: f64, new_full_end: u64) {
    let old_full_end = Composition::checked_frame_count(comp.fps, comp.duration);
    if comp.work_area_in == 0 && old_full_end == Some(comp.work_area_out) {
        comp.work_area_out = new_full_end;
        return;
    }

    // Work areas are half-open frame ranges. For a valid existing FPS, keep
    // both boundaries at the same times and quantize them to the closest new
    // frame. A malformed legacy/in-memory FPS has no meaningful time basis;
    // retaining and clamping its frame indices still lets this update repair
    // the Composition instead of making it permanently uneditable.
    let map_boundary = |frame: u64| {
        let mapped = if comp.fps.is_finite() && comp.fps > 0.0 {
            ((frame as f64 / comp.fps) * fps).round()
        } else {
            frame as f64
        };
        (mapped.max(0.0) as u64).min(new_full_end)
    };
    let new_in = map_boundary(comp.work_area_in);
    let new_out = map_boundary(comp.work_area_out).max(new_in);
    comp.work_area_in = new_in;
    comp.work_area_out = new_out;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project_with_composition() -> (Project, Uuid) {
        let mut project = Project::new("composition handler");
        let (composition, track) = Composition::new("main", 1920, 1080, 30.0, 10.0);
        let composition_id = composition.id;
        project
            .add_track(track)
            .expect("container structural Merge insertion must succeed");
        project
            .add_composition(composition)
            .expect("container structural Merge insertion must succeed");
        (project, composition_id)
    }

    #[test]
    fn add_and_update_reject_invalid_settings_atomically() {
        let (original, composition_id) = project_with_composition();
        let invalid = [
            (0, 1080, 30.0, 10.0),
            (1920, 0, 30.0, 10.0),
            (1920, 1080, 0.0, 10.0),
            (1920, 1080, f64::NAN, 10.0),
            (1920, 1080, 30.0, -1.0),
            (1920, 1080, 30.0, f64::INFINITY),
            (1920, 1080, f64::MAX, f64::MAX),
        ];

        for (width, height, fps, duration) in invalid {
            let shared = Arc::new(RwLock::new(original.clone()));
            assert!(matches!(
                CompositionHandler::add_composition(
                    &shared, "invalid", width, height, fps, duration
                ),
                Err(LibraryError::Validation(_))
            ));
            assert_eq!(*shared.read().unwrap(), original);

            let shared = Arc::new(RwLock::new(original.clone()));
            assert!(matches!(
                CompositionHandler::update_composition(
                    &shared,
                    composition_id,
                    "invalid",
                    width as u32,
                    height as u32,
                    fps,
                    duration,
                ),
                Err(LibraryError::Validation(_))
            ));
            assert_eq!(*shared.read().unwrap(), original);
        }
    }

    #[test]
    fn frame_count_rejects_the_saturating_boundary_but_accepts_representable_values() {
        assert_eq!(Composition::checked_frame_count(1.0, u64::MAX as f64), None);
        assert_eq!(
            Composition::checked_frame_count(1.0, (u64::MAX as f64) / 2.0),
            Some(1_u64 << 63)
        );
    }

    #[test]
    fn full_work_area_follows_new_duration_and_fps() {
        let (project, composition_id) = project_with_composition();
        let shared = Arc::new(RwLock::new(project));

        CompositionHandler::update_composition(
            &shared,
            composition_id,
            "main",
            1920,
            1080,
            60.0,
            12.5,
        )
        .unwrap();

        let project = shared.read().unwrap();
        let composition = project.get_composition(composition_id).unwrap();
        assert_eq!(
            (composition.work_area_in, composition.work_area_out),
            (0, 750)
        );
    }

    #[test]
    fn custom_work_area_preserves_seconds_then_clamps_to_new_half_open_range() {
        let (mut project, composition_id) = project_with_composition();
        let composition = project.get_composition_mut(composition_id).unwrap();
        composition.work_area_in = 30;
        composition.work_area_out = 240;
        let shared = Arc::new(RwLock::new(project));

        CompositionHandler::update_composition(
            &shared,
            composition_id,
            "main",
            1920,
            1080,
            60.0,
            10.0,
        )
        .unwrap();
        {
            let project = shared.read().unwrap();
            let composition = project.get_composition(composition_id).unwrap();
            assert_eq!(
                (composition.work_area_in, composition.work_area_out),
                (60, 480)
            );
        }

        CompositionHandler::update_composition(
            &shared,
            composition_id,
            "main",
            1920,
            1080,
            60.0,
            4.0,
        )
        .unwrap();
        let project = shared.read().unwrap();
        let composition = project.get_composition(composition_id).unwrap();
        assert_eq!(
            (composition.work_area_in, composition.work_area_out),
            (60, 240)
        );
    }
}

//! Minimal graph fixture for physical Color Node creation and wiring QA.

use super::{E2E_COMPOSITION_ID, FixtureInfo};
use library::model::{Composition, Project};
use uuid::Uuid;

pub(super) const TRACK_ID: Uuid = Uuid::from_u128(0xc0_201);
pub(super) const PROJECT_NAME: &str = "RuViE Color Operations QA";

pub(super) fn install(project: &mut Project) -> Result<FixtureInfo, String> {
    project.name = PROJECT_NAME.to_string();

    let (mut composition, mut track) = Composition::new("Color Operations", 640, 360, 30.0, 4.0);
    composition.id = E2E_COMPOSITION_ID;
    composition.track_ids = vec![TRACK_ID];
    composition.ui_position = [0.0, 0.0];
    composition.ui_size = [1_400.0, 900.0];

    track.id = TRACK_ID;
    track.name = "Color Operations Track".to_string();
    track.ui_position = [80.0, 100.0];
    track.ui_size = [560.0, 360.0];

    project
        .add_track(track)
        .map_err(|error| format!("cannot insert Color operations Track: {error}"))?;
    project
        .add_composition(composition)
        .map_err(|error| format!("cannot insert Color operations Composition: {error}"))?;

    let connection_errors = project.validate_connections();
    if !connection_errors.is_empty() {
        return Err(format!(
            "Color operations QA fixture has invalid graph connections: {}",
            connection_errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }
    let containment_errors = project.validate_containment();
    if !containment_errors.is_empty() {
        return Err(format!(
            "Color operations QA fixture has invalid containment: {}",
            containment_errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; ")
        ));
    }

    Ok(FixtureInfo {
        composition_id: E2E_COMPOSITION_ID,
        expanded_tracks: vec![TRACK_ID],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_contains_only_one_empty_track_and_structural_nodes() {
        let mut project = Project::new("empty");
        let fixture = install(&mut project).unwrap();

        assert_eq!(project.name, PROJECT_NAME);
        assert_eq!(fixture.composition_id, E2E_COMPOSITION_ID);
        assert_eq!(fixture.expanded_tracks, vec![TRACK_ID]);
        assert_eq!(project.compositions.len(), 1);
        assert_eq!(project.tracks.len(), 1);
        assert_eq!(project.clips.len(), 0);
        assert_eq!(project.nodes.len(), 4);
        assert_eq!(project.compositions[0].track_ids, vec![TRACK_ID]);
        assert!(project.get_track(TRACK_ID).unwrap().clip_ids.is_empty());
        assert!(project.validate_connections().is_empty());
        assert!(project.validate_containment().is_empty());
    }
}

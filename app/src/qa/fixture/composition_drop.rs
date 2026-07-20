use super::FixtureInfo;
use library::model::{Composition, Project};
use uuid::Uuid;

pub const PARENT_COMPOSITION_ID: Uuid = Uuid::from_u128(0x91_00);
pub const PARENT_TRACK_ID: Uuid = Uuid::from_u128(0x91_01);
pub const SOURCE_COMPOSITION_ID: Uuid = Uuid::from_u128(0x92_00);
pub const SOURCE_TRACK_ID: Uuid = Uuid::from_u128(0x92_01);

pub(super) fn install(project: &mut Project) -> Result<FixtureInfo, String> {
    project.name = "Composition Drop QA".to_string();

    let (mut parent, mut parent_track) =
        Composition::new("Parent Composition", 640, 360, 30.0, 12.0);
    parent.id = PARENT_COMPOSITION_ID;
    parent_track.id = PARENT_TRACK_ID;
    parent.track_ids = vec![PARENT_TRACK_ID];

    let (mut source, mut source_track) =
        Composition::new("Reusable Composition", 320, 180, 24.0, 3.0);
    source.id = SOURCE_COMPOSITION_ID;
    source_track.id = SOURCE_TRACK_ID;
    source.track_ids = vec![SOURCE_TRACK_ID];

    project
        .add_track(parent_track)
        .expect("container structural Merge insertion must succeed");
    project
        .add_track(source_track)
        .expect("container structural Merge insertion must succeed");
    project
        .add_composition(parent)
        .expect("container structural Merge insertion must succeed");
    project
        .add_composition(source)
        .expect("container structural Merge insertion must succeed");

    Ok(FixtureInfo {
        composition_id: PARENT_COMPOSITION_ID,
        expanded_tracks: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixture_keeps_source_composition_outside_parent_track() {
        let mut project = Project::new("empty");
        let fixture = install(&mut project).unwrap();

        assert_eq!(fixture.composition_id, PARENT_COMPOSITION_ID);
        assert_eq!(project.compositions.len(), 2);
        assert_eq!(
            project
                .get_composition(PARENT_COMPOSITION_ID)
                .unwrap()
                .track_ids,
            vec![PARENT_TRACK_ID]
        );
        assert_eq!(
            project
                .get_composition(SOURCE_COMPOSITION_ID)
                .unwrap()
                .track_ids,
            vec![SOURCE_TRACK_ID]
        );
        assert!(project
            .get_track(PARENT_TRACK_ID)
            .unwrap()
            .clip_ids
            .is_empty());
    }
}

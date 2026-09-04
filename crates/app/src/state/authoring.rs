//! Transient UI state for the authoritative Timeline editing model.
//!
//! None of these values are persisted as Project data. Gestures retain their
//! immutable model origin and expose only a visual projection until release;
//! the editor service therefore receives one atomic command per gesture.

use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::time::Instant;

use library::editor::{
    AuthoringPropertyOwner, AuthoringPropertyValueTarget, AuthoringPropertyValueUpdate,
};
use library::model::authoring::{
    AttachmentId, InstancePath, MediaTime, ModuleDefinitionId, ProjectRevision, TimelineId,
    TimelineInterval, TimelineItemId, TimelineTrackId, TransitionId,
};
use library::model::frame::transform::Transform;
use library::model::property::Vec2 as PropertyVec2;
use library::rendering::renderer::Affine2D;
use pan_zoom_ui::CanvasTransform;

use crate::model::ui_types::GizmoHandle;

use super::node_editor::NodeEditorState;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AuthoringSelection {
    Timeline(TimelineId),
    Track(TimelineTrackId),
    Item(TimelineItemId),
    Transition(TransitionId),
    Asset(uuid::Uuid),
    ModuleDefinition(ModuleDefinitionId),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AssetBrowserViewMode {
    #[default]
    List,
    Table,
    Grid,
}

impl AssetBrowserViewMode {
    pub const fn qa_name(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Table => "table",
            Self::Grid => "grid",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct AuthoringAssetBrowserState {
    pub view_mode: AssetBrowserViewMode,
}

#[derive(Clone, Debug, Default)]
pub struct AuthoringSelectionState {
    selected: Vec<AuthoringSelection>,
}

impl AuthoringSelectionState {
    pub fn primary(&self) -> Option<AuthoringSelection> {
        self.selected.last().copied()
    }

    pub fn contains(&self, selection: AuthoringSelection) -> bool {
        self.selected.contains(&selection)
    }

    pub fn iter(&self) -> impl Iterator<Item = AuthoringSelection> + '_ {
        self.selected.iter().copied()
    }

    pub fn replace(&mut self, selection: AuthoringSelection) {
        self.selected.clear();
        self.selected.push(selection);
    }

    pub fn add(&mut self, selection: AuthoringSelection) {
        if !self.contains(selection) {
            self.selected.push(selection);
        }
    }

    pub fn remove(&mut self, selection: AuthoringSelection) -> bool {
        let original_len = self.selected.len();
        self.selected.retain(|candidate| *candidate != selection);
        self.selected.len() != original_len
    }

    pub fn toggle(&mut self, selection: AuthoringSelection) {
        if !self.remove(selection) {
            self.selected.push(selection);
        }
    }

    pub fn make_primary(&mut self, selection: AuthoringSelection) -> bool {
        if !self.remove(selection) {
            return false;
        }
        self.selected.push(selection);
        true
    }

    pub fn clear(&mut self) {
        self.selected.clear();
    }

    pub fn retain(&mut self, mut keep: impl FnMut(AuthoringSelection) -> bool) {
        self.selected.retain(|selection| keep(*selection));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimelineGestureKind {
    Move,
    TrimStart,
    TrimEnd,
}

/// Immutable origin plus the current visual projection of one clip gesture.
#[derive(Clone, Debug)]
pub struct TimelineItemGesture {
    pub item_id: TimelineItemId,
    pub kind: TimelineGestureKind,
    pub pointer_origin: egui::Pos2,
    pub original_track_id: TimelineTrackId,
    pub original_layer: i64,
    pub original_interval: TimelineInterval,
    pub projected_track_id: TimelineTrackId,
    pub projected_layer: i64,
    pub projected_interval: TimelineInterval,
}

impl TimelineItemGesture {
    pub fn changed(&self) -> bool {
        self.projected_track_id != self.original_track_id
            || self.projected_layer != self.original_layer
            || self.projected_interval != self.original_interval
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthoringLibraryDrag {
    Asset(uuid::Uuid),
    Timeline(TimelineId),
    ModuleDefinition(ModuleDefinitionId),
    NewNodeClip,
}

#[derive(Clone, Debug)]
pub struct AuthoringTimelineView {
    pub current_frame: i64,
    pub is_playing: bool,
    pub pixels_per_second: f32,
    /// Vertical scale of the shared Timeline canvas. Every row consumer uses
    /// `TimelineRowMetrics`; this value is never applied independently by a
    /// painter or hit-test path.
    pub vertical_zoom: f32,
    pub horizontal_scroll: f32,
    pub vertical_scroll: f32,
    pub expanded_tracks: HashSet<TimelineTrackId>,
    /// Default clip-bar presentation for each Track. Missing entries use
    /// Content so media thumbnails and waveforms remain the normal view.
    pub track_display_modes: HashMap<TimelineTrackId, TimelineClipDisplayMode>,
    /// Per-Clip overrides of the containing Track presentation.
    pub item_display_modes: HashMap<TimelineItemId, TimelineClipDisplayMode>,
    /// Clips whose item-owned automation lanes are visible as Dope Sheet
    /// rows. This is presentation state and never changes Project ownership.
    pub expanded_items: HashSet<TimelineItemId>,
    pub item_gesture: Option<TimelineItemGesture>,
    pub keyframe_gesture: Option<TimelineKeyframeGesture>,
    pub library_drag: Option<AuthoringLibraryDrag>,
    pub playback_anchor: Option<(Instant, i64)>,
}

impl Default for AuthoringTimelineView {
    fn default() -> Self {
        Self {
            current_frame: 0,
            is_playing: false,
            pixels_per_second: 80.0,
            vertical_zoom: 1.0,
            horizontal_scroll: 0.0,
            vertical_scroll: 0.0,
            expanded_tracks: HashSet::new(),
            track_display_modes: HashMap::new(),
            item_display_modes: HashMap::new(),
            expanded_items: HashSet::new(),
            item_gesture: None,
            keyframe_gesture: None,
            library_drag: None,
            playback_anchor: None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TimelineClipDisplayMode {
    #[default]
    Content,
    Keyframes,
}

impl TimelineClipDisplayMode {
    pub const fn toggled(self) -> Self {
        match self {
            Self::Content => Self::Keyframes,
            Self::Keyframes => Self::Content,
        }
    }

    pub const fn qa_name(self) -> &'static str {
        match self {
            Self::Content => "content",
            Self::Keyframes => "keyframes",
        }
    }
}

/// Immutable keyframe origin plus a Timeline-only horizontal projection.
/// The model receives one atomic update when the pointer is released.
#[derive(Clone, Debug)]
pub struct TimelineKeyframeGesture {
    /// Clip row used only as the Timeline presentation anchor. Automation
    /// ownership is carried separately by `lane`.
    pub anchor_item_id: TimelineItemId,
    pub lane: AutomationLaneId,
    pub keyframe_id: library::model::property::KeyframeId,
    pub pointer_origin_x: f32,
    pub original_time: MediaTime,
    pub projected_time: MediaTime,
}

impl TimelineKeyframeGesture {
    pub fn changed(&self) -> bool {
        self.projected_time != self.original_time
    }
}

impl AuthoringTimelineView {
    pub fn track_display_mode(&self, track_id: TimelineTrackId) -> TimelineClipDisplayMode {
        self.track_display_modes
            .get(&track_id)
            .copied()
            .unwrap_or_default()
    }

    pub fn item_display_mode(
        &self,
        item_id: TimelineItemId,
        track_id: TimelineTrackId,
    ) -> TimelineClipDisplayMode {
        self.item_display_modes
            .get(&item_id)
            .copied()
            .unwrap_or_else(|| self.track_display_mode(track_id))
    }

    pub fn shows_property_rows(&self, item_id: TimelineItemId, track_id: TimelineTrackId) -> bool {
        self.expanded_items.contains(&item_id)
            || self.item_display_mode(item_id, track_id) == TimelineClipDisplayMode::Keyframes
    }

    pub fn seek_frame(&mut self, frame: i64) {
        self.current_frame = frame.max(0);
        if self.is_playing {
            self.playback_anchor = Some((Instant::now(), self.current_frame));
        }
    }

    pub fn set_playing(&mut self, playing: bool) {
        if self.is_playing == playing {
            return;
        }
        self.is_playing = playing;
        self.playback_anchor = playing.then(|| (Instant::now(), self.current_frame));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreviewTool {
    Select,
    Text,
    Path,
    Pan,
    Zoom,
}

/// Immutable authoring origin plus the current canvas-only transform preview.
/// The Project is updated exactly once when the pointer is released.
#[derive(Clone, Debug)]
pub(crate) struct PreviewTransformGesture {
    pub item_id: TimelineItemId,
    /// `None` is body translation; `Some` identifies a production resize or
    /// rotation handle.
    pub handle: Option<GizmoHandle>,
    pub pointer_origin: egui::Pos2,
    pub canvas_origin: CanvasTransform,
    pub original_position: PropertyVec2,
    pub projected_position: PropertyVec2,
    pub original_scale: PropertyVec2,
    pub projected_scale: PropertyVec2,
    pub original_rotation: f64,
    pub projected_rotation: f64,
    /// Evaluated transform and hierarchy captured from the rendered FrameInfo.
    /// These keep transient outlines attached to the pixels the user grabbed,
    /// including parent transforms and binding-adjusted presentation.
    pub original_visual_transform: Transform,
    pub projected_visual_transform: Transform,
    pub parent_transform: Affine2D,
    pub local_bounds: egui::Rect,
    pub local_time: MediaTime,
    pub position_keyframed: bool,
    pub scale_keyframed: bool,
    pub rotation_keyframed: bool,
    pub project_revision: ProjectRevision,
}

#[derive(Clone)]
pub struct AuthoringPreviewView {
    /// One canonical transform shared by Preview content, grid, navigation,
    /// direct manipulation, and QA geometry.
    pub canvas: pan_zoom_ui::CanvasState,
    pub auto_fit: bool,
    pub show_grid: bool,
    pub active_tool: PreviewTool,
    pub fitted_timeline: Option<TimelineId>,
    pub last_viewport_size: egui::Vec2,
    pub texture: Option<egui::TextureHandle>,
    pub texture_width: u32,
    pub texture_height: u32,
    pub rendered_revision: Option<u64>,
    pub rendered_frame: Option<i64>,
    pub nontransparent_pixels: Option<u64>,
    pub pixel_hash: Option<u64>,
    pub(crate) transform_gesture: Option<PreviewTransformGesture>,
    pub(crate) text_editor: super::text_editor::TextEditorState,
    pub(crate) path_editor: super::path_editor::PathEditorState,
}

impl Default for AuthoringPreviewView {
    fn default() -> Self {
        Self {
            canvas: pan_zoom_ui::CanvasState::uniform(egui::Vec2::ZERO, 1.0),
            auto_fit: true,
            show_grid: true,
            active_tool: PreviewTool::Select,
            fitted_timeline: None,
            last_viewport_size: egui::Vec2::ZERO,
            texture: None,
            texture_width: 0,
            texture_height: 0,
            rendered_revision: None,
            rendered_frame: None,
            nontransparent_pixels: None,
            pixel_hash: None,
            transform_gesture: None,
            text_editor: super::text_editor::TextEditorState::default(),
            path_editor: super::path_editor::PathEditorState::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct CurveKeyDrag {
    pub lane: AutomationLaneId,
    pub component: CurveValueComponent,
    pub keyframe_id: library::model::property::KeyframeId,
    pub original_time: MediaTime,
    pub original_value: library::model::property::PropertyValue,
    pub projected_time: MediaTime,
    pub projected_value: library::model::property::PropertyValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CurveValueComponent {
    Scalar,
    X,
    Y,
    Z,
    W,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AutomationTarget {
    AuthoredProperty(String),
    ModuleParameter(library::model::authoring::PublishedParameterId),
    AttachmentParameter {
        attachment_id: AttachmentId,
        key: String,
    },
}

/// Authoritative owner of one automation lane. A Transition definition and a
/// concrete nested placement deliberately remain different identities.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AutomationOwner {
    Item(TimelineItemId),
    TransitionDefinition(TransitionId),
    TransitionInstance {
        transition_id: TransitionId,
        instance_path: InstancePath,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AutomationLaneId {
    pub owner: AutomationOwner,
    pub target: AutomationTarget,
}

#[derive(Clone, Debug)]
pub struct CurveEditorState {
    pub target_owner: Option<AutomationOwner>,
    /// One canonical transform shared by curve content, grid, hit testing,
    /// playhead, navigation, and QA geometry.
    pub canvas: pan_zoom_ui::CanvasState,
    pub visible_lanes: HashSet<AutomationLaneId>,
    pub drag: Option<CurveKeyDrag>,
}

#[derive(Clone, Debug, Default)]
pub struct AuthoringInspectorView {
    pub target: Option<AuthoringSelection>,
    /// Revision from which the editable draft was populated. Drafts stay
    /// stable during a gesture, then refresh after any committed edit/undo.
    pub synced_revision: Option<ProjectRevision>,
    /// Playhead used for the currently displayed effective automation values.
    /// Unlike a project revision, seeking changes this value every frame.
    pub synced_frame: Option<i64>,
    pub name: String,
    pub text: String,
    pub start_seconds: f64,
    pub duration_seconds: f64,
    pub property_values: std::collections::HashMap<String, library::model::property::PropertyValue>,
    /// Editable Python source keyed by the same stable Inspector control ID as
    /// its authored property. Source drafts survive focus changes and commit as
    /// one Timeline edit rather than one transaction per keystroke.
    pub expression_sources: std::collections::HashMap<String, String>,
    pub effect_values:
        std::collections::HashMap<(AttachmentId, String), library::model::property::PropertyValue>,
    /// One shared typed-value projection produced by an Inspector drag. The
    /// Preview may apply it to an immutable Project snapshot while release
    /// submits the same update as one service transaction.
    pub(crate) transient_property_edit: Option<TransientPropertyEdit>,
}

impl AuthoringInspectorView {
    pub fn invalidate(&mut self) {
        self.target = None;
        self.synced_revision = None;
        self.synced_frame = None;
        self.property_values.clear();
        self.expression_sources.clear();
        self.effect_values.clear();
        self.transient_property_edit = None;
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TransientPropertyEdit {
    pub(crate) source_revision: ProjectRevision,
    pub(crate) owner: AuthoringPropertyOwner,
    pub(crate) update: AuthoringPropertyValueUpdate,
}

impl TransientPropertyEdit {
    pub(crate) fn digest(&self) -> u64 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.source_revision.get().hash(&mut hasher);
        self.owner.hash(&mut hasher);
        self.update.key.hash(&mut hasher);
        self.update.value.hash(&mut hasher);
        match self.update.target {
            AuthoringPropertyValueTarget::Constant => 0_u8.hash(&mut hasher),
            AuthoringPropertyValueTarget::Keyframe { local_time } => {
                1_u8.hash(&mut hasher);
                local_time.hash(&mut hasher);
            }
        }
        hasher.finish()
    }

    pub(crate) fn matches(&self, owner: AuthoringPropertyOwner, key: &str) -> bool {
        self.owner == owner && self.update.key == key
    }
}

impl Default for CurveEditorState {
    fn default() -> Self {
        Self {
            target_owner: None,
            canvas: pan_zoom_ui::CanvasState::uniform(egui::Vec2::ZERO, 1.0),
            visible_lanes: HashSet::new(),
            drag: None,
        }
    }
}

pub struct AuthoringUiState {
    pub active_timeline_id: TimelineId,
    /// Concrete placement path used to disambiguate repeated nested
    /// compositions. The root id always remains the Project root Timeline.
    pub active_instance_path: Option<InstancePath>,
    pub selection: AuthoringSelectionState,
    pub assets: AuthoringAssetBrowserState,
    pub timeline: AuthoringTimelineView,
    pub preview: AuthoringPreviewView,
    pub curve_editor: CurveEditorState,
    pub inspector: AuthoringInspectorView,
    pub node_editor: NodeEditorState,
    pub error: Option<String>,
    pub status: String,
}

impl AuthoringUiState {
    pub fn new(root_timeline_id: TimelineId) -> Self {
        Self {
            active_timeline_id: root_timeline_id,
            active_instance_path: Some(InstancePath::root(root_timeline_id)),
            selection: AuthoringSelectionState::default(),
            assets: AuthoringAssetBrowserState::default(),
            timeline: AuthoringTimelineView::default(),
            preview: AuthoringPreviewView::default(),
            curve_editor: CurveEditorState::default(),
            inspector: AuthoringInspectorView::default(),
            node_editor: NodeEditorState::default(),
            error: None,
            status: "Ready".to_string(),
        }
    }

    pub fn reconcile(&mut self, project: &library::model::authoring::AuthoringProject) {
        let invalid_concrete_path = self.active_instance_path.as_ref().is_some_and(|path| {
            resolve_instance_path_timeline(project, path) != Some(self.active_timeline_id)
        });
        if !project.timelines.contains_key(&self.active_timeline_id) || invalid_concrete_path {
            self.active_timeline_id = project.root_timeline_id;
            self.active_instance_path = Some(InstancePath::root(project.root_timeline_id));
            self.preview.auto_fit = true;
        }
        self.selection.retain(|selection| match selection {
            AuthoringSelection::Timeline(id) => project.timelines.contains_key(&id),
            AuthoringSelection::Track(id) => project.tracks.contains_key(&id),
            AuthoringSelection::Item(id) => project.items.contains_key(&id),
            AuthoringSelection::Transition(id) => project.transitions.contains_key(&id),
            AuthoringSelection::Asset(id) => project.assets.iter().any(|asset| asset.id == id),
            AuthoringSelection::ModuleDefinition(id) => {
                project.module_definitions.contains_key(&id)
            }
        });
        if self.preview.path_editor.target_item.is_some_and(|item_id| {
            !project.items.contains_key(&item_id)
                || self.selection.primary() != Some(AuthoringSelection::Item(item_id))
        }) {
            self.preview.path_editor.clear();
        }
        self.timeline.expanded_tracks.retain(|track_id| {
            project
                .tracks
                .get(track_id)
                .is_some_and(|track| track.timeline_id == self.active_timeline_id)
        });
        self.timeline.expanded_items.retain(|item_id| {
            project.items.get(item_id).is_some_and(|item| {
                project
                    .tracks
                    .get(&item.track_id)
                    .is_some_and(|track| track.timeline_id == self.active_timeline_id)
            })
        });
        self.timeline
            .track_display_modes
            .retain(|track_id, _| project.tracks.contains_key(track_id));
        self.timeline
            .item_display_modes
            .retain(|item_id, _| project.items.contains_key(item_id));
        if self
            .timeline
            .keyframe_gesture
            .as_ref()
            .is_some_and(|gesture| {
                !project.items.contains_key(&gesture.anchor_item_id)
                    || !automation_owner_exists(project, &gesture.lane.owner)
            })
        {
            self.timeline.keyframe_gesture = None;
        }
        if self
            .curve_editor
            .drag
            .as_ref()
            .is_some_and(|drag| !automation_owner_exists(project, &drag.lane.owner))
        {
            self.curve_editor.drag = None;
        }
        if self
            .preview
            .transform_gesture
            .as_ref()
            .is_some_and(|gesture| !project.items.contains_key(&gesture.item_id))
        {
            self.preview.transform_gesture = None;
        }
        let maximum_frame = project
            .timelines
            .get(&self.active_timeline_id)
            .and_then(|timeline| timeline.duration.checked_frame_index(timeline.fps).ok())
            .unwrap_or(0)
            .max(0);
        self.timeline.current_frame = self.timeline.current_frame.clamp(0, maximum_frame);
    }
}

fn resolve_instance_path_timeline(
    project: &library::model::authoring::AuthoringProject,
    path: &InstancePath,
) -> Option<TimelineId> {
    if path.root_timeline_id != project.root_timeline_id {
        return None;
    }
    let mut timeline_id = path.root_timeline_id;
    for item_id in &path.composition_items {
        let item = project.items.get(item_id)?;
        let track = project.tracks.get(&item.track_id)?;
        if track.timeline_id != timeline_id {
            return None;
        }
        let library::model::authoring::SourceRef::Composition(instance) = &item.source else {
            return None;
        };
        timeline_id = instance.timeline_id;
    }
    project
        .timelines
        .contains_key(&timeline_id)
        .then_some(timeline_id)
}

fn automation_owner_exists(
    project: &library::model::authoring::AuthoringProject,
    owner: &AutomationOwner,
) -> bool {
    match owner {
        AutomationOwner::Item(item_id) => project.items.contains_key(item_id),
        AutomationOwner::TransitionDefinition(transition_id) => {
            project.transitions.contains_key(transition_id)
        }
        AutomationOwner::TransitionInstance {
            transition_id,
            instance_path,
        } => project
            .resolve_transition_module_instance_target(instance_path, *transition_id)
            .is_ok(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use library::model::authoring::{AuthoringProject, RationalRate};

    #[test]
    fn item_gesture_does_not_mutate_its_origin() {
        let original =
            TimelineInterval::new(MediaTime::zero(), MediaTime::new(5, 1).unwrap()).unwrap();
        let mut gesture = TimelineItemGesture {
            item_id: TimelineItemId::new(),
            kind: TimelineGestureKind::Move,
            pointer_origin: egui::pos2(10.0, 10.0),
            original_track_id: TimelineTrackId::new(),
            original_layer: 0,
            original_interval: original,
            projected_track_id: TimelineTrackId::new(),
            projected_layer: 2,
            projected_interval: original,
        };
        gesture.projected_interval.start = MediaTime::new(3, 1).unwrap();
        assert_eq!(gesture.original_interval, original);
        assert!(gesture.changed());
    }

    #[test]
    fn reconcile_discards_only_stale_ui_references() {
        let project = AuthoringProject::new(
            "test",
            1920,
            1080,
            RationalRate::new(30, 1).unwrap(),
            MediaTime::new(10, 1).unwrap(),
        )
        .unwrap();
        let mut state = AuthoringUiState::new(project.root_timeline_id);
        state
            .selection
            .replace(AuthoringSelection::Item(TimelineItemId::new()));
        state.timeline.current_frame = 1_000;
        state.reconcile(&project);
        assert_eq!(state.selection.primary(), None);
        assert_eq!(state.timeline.current_frame, 300);
    }

    #[test]
    fn transient_property_digest_tracks_value_owner_and_keyframe_time() {
        let item_id = TimelineItemId::new();
        let operation_id = uuid::Uuid::new_v4();
        let edit = TransientPropertyEdit {
            source_revision: ProjectRevision::initial(),
            owner: AuthoringPropertyOwner::TextEnsemble {
                item_id,
                operation_id,
            },
            update: AuthoringPropertyValueUpdate {
                key: "tx".to_string(),
                value: library::model::property::PropertyValue::from(12.0),
                target: AuthoringPropertyValueTarget::Constant,
            },
        };
        let initial_digest = edit.digest();
        assert_eq!(initial_digest, edit.digest());

        let mut changed_value = edit.clone();
        changed_value.update.value = library::model::property::PropertyValue::from(-7.0);
        assert_ne!(edit.digest(), changed_value.digest());

        let mut changed_target = edit.clone();
        changed_target.update.target = AuthoringPropertyValueTarget::Keyframe {
            local_time: MediaTime::new(1, 2).unwrap(),
        };
        assert_ne!(edit.digest(), changed_target.digest());

        let mut changed_owner = edit.clone();
        changed_owner.owner = AuthoringPropertyOwner::TextEnsemble {
            item_id,
            operation_id: uuid::Uuid::new_v4(),
        };
        assert_ne!(edit.digest(), changed_owner.digest());
    }
}

use library::model::project::PortOwner;
use std::collections::BTreeMap;
use uuid::Uuid;

#[derive(Clone, Copy, Debug)]
pub(in crate::ui::panels::node_editor) enum LayoutEdit {
    MoveNode {
        node_id: Uuid,
        position: [f32; 2],
    },
    MoveContainer {
        owner: PortOwner,
        delta: [f32; 2],
    },
    ResizeContainer {
        owner: PortOwner,
        position: [f32; 2],
        size: [f32; 2],
    },
}

#[derive(Clone, Debug)]
pub(in crate::ui::panels::node_editor) enum AutoLayoutScope {
    All,
    Selection(Vec<Uuid>),
    Container(PortOwner),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::ui::panels::node_editor) struct ContainerLayout {
    pub(in crate::ui::panels::node_editor) position: [f32; 2],
    pub(in crate::ui::panels::node_editor) size: [f32; 2],
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(in crate::ui::panels::node_editor) struct AutoLayoutPlan {
    pub(in crate::ui::panels::node_editor) node_positions: BTreeMap<Uuid, [f32; 2]>,
    pub(in crate::ui::panels::node_editor) clip_layouts: BTreeMap<Uuid, ContainerLayout>,
    pub(in crate::ui::panels::node_editor) track_layouts: BTreeMap<Uuid, ContainerLayout>,
    pub(in crate::ui::panels::node_editor) composition_size: Option<[f32; 2]>,
}

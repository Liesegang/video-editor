use serde::{Deserialize, Serialize};
use uuid::Uuid;

macro_rules! define_id {
    ($name:ident) => {
        #[derive(
            Serialize, Deserialize, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Hash,
        )]
        #[serde(transparent)]
        pub struct $name(Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::new_v4())
            }

            pub const fn from_uuid(id: Uuid) -> Self {
                Self(id)
            }

            pub const fn as_uuid(self) -> Uuid {
                self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                self.0.fmt(formatter)
            }
        }
    };
}

define_id!(TimelineId);
define_id!(TimelineTrackId);
define_id!(TimelineItemId);
define_id!(TransitionId);
define_id!(CompositionParameterId);
define_id!(ModuleDefinitionId);
define_id!(ModuleInstanceId);
define_id!(ModuleConnectionId);
define_id!(PublishedParameterId);
define_id!(PublishedMediaInputId);
// Stable identity of a dedicated Module graph output terminal. This is not a
// Published Interface identity: render outputs are graph boundaries.
define_id!(ModuleOutputId);
define_id!(PublishedSignalId);
define_id!(PublishedActionId);
define_id!(AttachmentId);
define_id!(PaintDefinitionId);
define_id!(PaletteGroupId);

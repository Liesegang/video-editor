//! Editor services - public API for GUI interaction.
//!
//! This module contains all services that the GUI (app crate) should use
//! to interact with the library.

mod asset_import;
mod asset_preview;
pub mod audio_service;
mod authoring_factory;
mod authoring_qa_fixture;
mod authoring_waveform_service;
pub mod color_service;
pub mod editor_service;
pub mod handlers;
pub mod ocio_shim;
mod particle_node_clip;
pub mod project_model;
pub mod project_service;
pub mod render_service;
pub mod timeline_editor_service;

// Re-exports for convenient access
pub use crate::model::NodeGraphBundle;
pub use asset_preview::load_asset_preview_frame;
pub use audio_service::AudioService;
pub use authoring_factory::{
    AuthoringNodeFactory, BuiltinEffectFactory, ModuleNodeRequest, TextEnsembleOperationFactory,
    TextEnsembleOperationKind,
};
pub use authoring_qa_fixture::{
    AUTHORING_AUDIO_E2E_FIXTURE, AUTHORING_E2E_AUDIO, AUTHORING_E2E_FIXTURE, AUTHORING_E2E_IMAGE,
    AUTHORING_E2E_VIDEO, AUTHORING_PATH_E2E_FIXTURE, AuthoringAudioE2eFixture,
    AuthoringAudioE2eFixtureInfo, AuthoringE2eFixture, AuthoringE2eFixtureInfo,
    AuthoringPathE2eFixture, AuthoringPathE2eFixtureInfo, build_authoring_audio_e2e_fixture,
    build_authoring_e2e_fixture, build_authoring_path_e2e_fixture,
};
pub use authoring_waveform_service::AuthoringWaveformService;
pub use color_service::ColorSpaceManager as ColorService;
pub use editor_service::EditorService;
pub use handlers::clip_handler::ClipBundle;
pub use handlers::keyframe_handler::KeyframeBatchUpdate;
pub use handlers::property_ops::PropertyOwner;
pub use particle_node_clip::{
    ParticleNodeClipCreation, ParticleNodeClipDefinition, ParticleNodeClipFactory,
    ParticleNodeClipPlacement, ParticlePublishedParameters,
};
pub use project_model::ProjectModel;
pub use project_service::ProjectManager as ProjectService;
pub use render_service::{RenderDestination, RenderService};
pub use timeline_editor_service::{
    AuthoringKeyframeUpdate, AuthoringPropertyOwner, AuthoringPropertyValueTarget,
    AuthoringPropertyValueUpdate, EditPlan, EditPlanValidationScope, EditProjection,
    ModuleAttachmentPlacement, ModuleInputHost, ModuleInterfaceCommand, ModuleInterfaceEditImpact,
    ModuleInterfaceEditResult, ModuleItemPlacement, ModuleNodePresentationUpdate,
    NodeClipConversionResult, PreparedModuleDefinitionEdit, SharedModuleEdit, TimelineEditError,
    TimelineEditOperation, TimelineEditPlanningIndex, TimelineEditRequest, TimelineEditorService,
    TimelineItemDependency, TimelineItemEditState, TimelineSettingsUpdate,
    TransitionAutomationOwner, TransitionPlacement, plan_timeline_edit, project_edit_plan,
};

//! Clip-level Inspector projected from the authoritative Node graph.
//!
//! A Timeline Clip remains the selected identity. This module renders a
//! short-lived semantic projection and routes edits to either the exact Node,
//! the direct Clip, or the typed container facade. It never persists a second
//! model and never edits wires itself.

use std::sync::{Arc, RwLock};

use egui::{RichText, Ui};
use library::editor::project_service::{
    SemanticAnimationSupport, SemanticContainerPropertyStack, SemanticPropertyAccess,
    SemanticPropertyGroup, SemanticPropertyOwner, SemanticPropertySection,
};
use library::model::project::{NodeContainer, Project};
use library::model::property::{Property, PropertyMap, PropertyUiType, PropertyValue};
use library::model::Clip;
use library::{EditorService, PropertyOwner};

use super::clip_timing::render_clip_timing;
use super::evaluation::{evaluate_property_map, render_evaluation_issues};
use super::properties::{render_property_rows, PropertyRenderContext};
use super::property_authoring::PropertyAction;
use crate::action::HistoryManager;
use crate::state::context::EditorContext;
use crate::ui::widgets::property_mode::property_for_mode;

mod source_color;
mod stack_controls;

#[allow(
    clippy::too_many_arguments,
    reason = "the semantic Clip projection needs the same timing and authoring context as exact Node properties"
)]
pub(super) fn render(
    ui: &mut Ui,
    clip: &Clip,
    current_time: f64,
    fps: f64,
    resolution: (u64, u64),
    project_service: &mut EditorService,
    history_manager: &mut HistoryManager,
    editor_context: &EditorContext,
    project: &Arc<RwLock<Project>>,
    needs_refresh: &mut bool,
) {
    let owner = NodeContainer::Clip(clip.id);
    let stack = match project_service.semantic_container_property_stack(owner) {
        Ok(stack) => stack,
        Err(error) => {
            render_root_error(ui, clip.id, &error.to_string());
            return;
        }
    };

    let root = ui.scope(|ui| {
        render_stack_diagnostics(ui, clip.id, &stack);
        source_color::render(ui, clip.id, project_service, history_manager, needs_refresh);
        stack_controls::render(
            ui,
            clip.id,
            &stack,
            project_service,
            history_manager,
            needs_refresh,
        );
        for section in stack.sections() {
            if section.group() == SemanticPropertyGroup::Timing {
                let response = ui.scope(|ui| {
                    render_clip_timing(
                        ui,
                        clip,
                        fps,
                        project_service,
                        history_manager,
                        project,
                        needs_refresh,
                    );
                });
                register_section(clip.id, section, response.response.rect, true);
                continue;
            }
            render_section(
                ui,
                clip.id,
                section,
                current_time,
                fps,
                resolution,
                project_service,
                history_manager,
                editor_context,
                needs_refresh,
            );
        }
    });
    crate::qa::register_component_with_metadata(
        format!("inspector.semantic.clip:{}", clip.id),
        "inspector_semantic_clip",
        root.response.rect,
        true,
        Some(serde_json::json!({
            "owner": "clip",
            "clip_id": clip.id,
            "selection_identity": "clip",
            "section_count": stack.sections().len(),
            "diagnostic_count": stack.diagnostics().len(),
            "projection_persisted": false,
        })),
    );
}

#[allow(
    clippy::too_many_arguments,
    reason = "each semantic section shares typed editing, evaluation, history, and QA context"
)]
fn render_section(
    ui: &mut Ui,
    clip_id: uuid::Uuid,
    section: &SemanticPropertySection,
    current_time: f64,
    fps: f64,
    resolution: (u64, u64),
    project_service: &mut EditorService,
    history_manager: &mut HistoryManager,
    editor_context: &EditorContext,
    needs_refresh: &mut bool,
) {
    ui.add_space(8.0);
    let response = egui::CollapsingHeader::new(section.label())
        .id_salt(("semantic_clip_section", clip_id, section.stable_id()))
        .default_open(default_open(section.group()))
        .show(ui, |ui| {
            render_section_diagnostics(ui, clip_id, section);
            render_section_properties(
                ui,
                clip_id,
                section,
                current_time,
                fps,
                resolution,
                project_service,
                history_manager,
                editor_context,
                needs_refresh,
            );
        });
    register_section(
        clip_id,
        section,
        response.header_response.rect,
        response.body_response.is_some(),
    );
}

#[allow(
    clippy::too_many_arguments,
    reason = "property rows preserve the existing Inspector authoring controls while routing by semantic owner"
)]
fn render_section_properties(
    ui: &mut Ui,
    clip_id: uuid::Uuid,
    section: &SemanticPropertySection,
    current_time: f64,
    fps: f64,
    resolution: (u64, u64),
    project_service: &mut EditorService,
    history_manager: &mut HistoryManager,
    editor_context: &EditorContext,
    needs_refresh: &mut bool,
) {
    let mut properties = PropertyMap::new();
    for entry in section.properties() {
        properties.set(entry.key().to_string(), entry.property().clone());
    }
    let qa_scope = format!("semantic.clip:{clip_id}.section:{}", section.stable_id());
    let evaluated =
        evaluate_property_map(project_service, &properties, current_time, fps, resolution);
    render_evaluation_issues(ui, &qa_scope, evaluated.issues());

    for entry in section.properties() {
        let (editable, show_authoring) = property_capabilities(entry.access(), entry.animation());
        let Some(definition) = entry.definition() else {
            let response = ui.label(format!("{} · Unsupported value metadata", entry.label()));
            register_property_access(
                clip_id,
                section,
                entry.key(),
                entry.access(),
                response.rect,
                false,
            );
            continue;
        };
        let context = PropertyRenderContext {
            available_fonts: &editor_context.available_fonts,
            in_grid: !matches!(definition.ui_type(), PropertyUiType::MultilineText),
            current_time,
            show_authoring,
            qa_scope: qa_scope.clone(),
        };
        let response = ui.add_enabled_ui(editable, |ui| {
            if context.in_grid {
                egui::Grid::new((
                    "semantic_clip_property",
                    clip_id,
                    section.stable_id(),
                    entry.key(),
                ))
                .striped(true)
                .show(ui, |ui| {
                    render_property_rows(
                        ui,
                        std::slice::from_ref(definition),
                        |name| evaluated.value(name).cloned(),
                        |name| properties.get(name).cloned(),
                        &context,
                    )
                })
                .inner
            } else {
                render_property_rows(
                    ui,
                    std::slice::from_ref(definition),
                    |name| evaluated.value(name).cloned(),
                    |name| properties.get(name).cloned(),
                    &context,
                )
            }
        });
        register_property_access(
            clip_id,
            section,
            entry.key(),
            entry.access(),
            response.response.rect,
            editable,
        );
        render_access_note(ui, clip_id, section, entry.key(), entry.access());
        if editable {
            let mut actions = SemanticPropertyActions::new(
                project_service,
                history_manager,
                section.owner(),
                current_time,
            );
            let errors = actions.handle(response.inner, |name| properties.get(name).cloned());
            *needs_refresh |= actions.changed;
            for error in errors {
                render_action_error(ui, clip_id, section.stable_id(), entry.key(), &error);
            }
        }
    }
}

struct SemanticPropertyActions<'a> {
    project_service: &'a mut EditorService,
    history_manager: &'a mut HistoryManager,
    owner: SemanticPropertyOwner,
    current_time: f64,
    changed: bool,
}

impl<'a> SemanticPropertyActions<'a> {
    fn new(
        project_service: &'a mut EditorService,
        history_manager: &'a mut HistoryManager,
        owner: SemanticPropertyOwner,
        current_time: f64,
    ) -> Self {
        Self {
            project_service,
            history_manager,
            owner,
            current_time,
            changed: false,
        }
    }

    fn handle(
        &mut self,
        actions: Vec<PropertyAction>,
        get_property: impl Fn(&str) -> Option<Property>,
    ) -> Vec<String> {
        let mut errors = Vec::new();
        for action in actions {
            let commit_policy = action_commit_policy(&action);
            let result = match action {
                PropertyAction::Update(name, value) => self.update(&name, value),
                PropertyAction::UpdateGroup(_) => Err(library::LibraryError::Validation(
                    "Grouped direct-Node property updates are not valid in the semantic Clip facade"
                        .to_string(),
                )),
                PropertyAction::Commit => {
                    self.commit_history();
                    continue;
                }
                PropertyAction::ToggleKeyframe(name, value) => {
                    let property = get_property(&name);
                    self.toggle_keyframe(&name, value, property.as_ref())
                }
                PropertyAction::SetAttribute(name, key, value) => {
                    self.set_attribute(&name, &key, value)
                }
                PropertyAction::SetMode(name, mode, value) => {
                    let replacement = property_for_mode(
                        get_property(&name).as_ref(),
                        mode,
                        value,
                        self.current_time,
                    )
                    .map_err(library::LibraryError::Project);
                    replacement.and_then(|property| self.replace(&name, property))
                }
                PropertyAction::SetExpressionSource(name, source) => {
                    self.set_expression_source(&name, source)
                }
            };
            match result {
                Ok(()) => {
                    self.changed = true;
                    if matches!(commit_policy, PropertyActionCommit::Immediate) {
                        self.commit_history();
                    }
                }
                Err(error) => errors.push(error.to_string()),
            }
        }
        errors
    }

    fn update(&self, key: &str, value: PropertyValue) -> Result<(), library::LibraryError> {
        match self.owner {
            SemanticPropertyOwner::DirectClip(id) => {
                self.project_service.update_property_or_keyframe(
                    PropertyOwner::Clip(id),
                    key,
                    self.current_time,
                    value,
                    None,
                )
            }
            SemanticPropertyOwner::ExactNode(id) => {
                self.project_service.update_property_or_keyframe(
                    PropertyOwner::Node(id),
                    key,
                    self.current_time,
                    value,
                    None,
                )
            }
            SemanticPropertyOwner::SemanticContainer(owner) => self
                .project_service
                .update_semantic_container_property_or_keyframe(
                    owner,
                    key,
                    self.current_time,
                    value,
                    None,
                ),
        }
    }

    fn replace(&self, key: &str, property: Property) -> Result<(), library::LibraryError> {
        match self.owner {
            SemanticPropertyOwner::DirectClip(id) => {
                self.project_service
                    .replace_property(PropertyOwner::Clip(id), key, property)
            }
            SemanticPropertyOwner::ExactNode(id) => {
                self.project_service
                    .replace_property(PropertyOwner::Node(id), key, property)
            }
            SemanticPropertyOwner::SemanticContainer(owner) => self
                .project_service
                .replace_semantic_container_property(owner, key, property),
        }
    }

    fn set_attribute(
        &self,
        key: &str,
        attribute: &str,
        value: PropertyValue,
    ) -> Result<(), library::LibraryError> {
        match self.owner {
            SemanticPropertyOwner::DirectClip(id) => self.project_service.set_property_attribute(
                PropertyOwner::Clip(id),
                key,
                attribute,
                value,
            ),
            SemanticPropertyOwner::ExactNode(id) => self.project_service.set_property_attribute(
                PropertyOwner::Node(id),
                key,
                attribute,
                value,
            ),
            SemanticPropertyOwner::SemanticContainer(owner) => self
                .project_service
                .set_semantic_container_property_attribute(
                    owner,
                    key,
                    attribute.to_string(),
                    value,
                ),
        }
    }

    fn set_expression_source(
        &self,
        key: &str,
        source: String,
    ) -> Result<(), library::LibraryError> {
        match self.owner {
            SemanticPropertyOwner::DirectClip(id) => {
                self.project_service
                    .set_expression_source(PropertyOwner::Clip(id), key, source)
            }
            SemanticPropertyOwner::ExactNode(id) => {
                self.project_service
                    .set_expression_source(PropertyOwner::Node(id), key, source)
            }
            SemanticPropertyOwner::SemanticContainer(owner) => self
                .project_service
                .set_semantic_container_property_attribute(
                    owner,
                    key,
                    "expression".to_string(),
                    PropertyValue::String(source),
                ),
        }
    }

    fn toggle_keyframe(
        &self,
        key: &str,
        value: PropertyValue,
        property: Option<&Property>,
    ) -> Result<(), library::LibraryError> {
        const TOLERANCE: f64 = 0.001;
        let keyframe =
            property.and_then(|property| property.keyframe_id_at(self.current_time, TOLERANCE));
        if let Some(keyframe) = keyframe {
            match self.owner {
                SemanticPropertyOwner::DirectClip(id) => self
                    .project_service
                    .remove_keyframe_by_id(PropertyOwner::Clip(id), key, keyframe),
                SemanticPropertyOwner::ExactNode(id) => self.project_service.remove_keyframe_by_id(
                    PropertyOwner::Node(id),
                    key,
                    keyframe,
                ),
                SemanticPropertyOwner::SemanticContainer(owner) => self
                    .project_service
                    .remove_semantic_container_keyframe_by_id(owner, key, keyframe),
            }
        } else {
            match self.owner {
                SemanticPropertyOwner::DirectClip(id) => self
                    .project_service
                    .add_keyframe_with_id(
                        PropertyOwner::Clip(id),
                        key,
                        self.current_time,
                        value,
                        None,
                    )
                    .map(|_| ()),
                SemanticPropertyOwner::ExactNode(id) => self
                    .project_service
                    .add_keyframe_with_id(
                        PropertyOwner::Node(id),
                        key,
                        self.current_time,
                        value,
                        None,
                    )
                    .map(|_| ()),
                SemanticPropertyOwner::SemanticContainer(owner) => self
                    .project_service
                    .add_semantic_container_keyframe(owner, key, self.current_time, value, None)
                    .map(|_| ()),
            }
        }
    }

    fn commit_history(&mut self) {
        match self.project_service.get_project().read() {
            Ok(project) => self.history_manager.push_project_state(project.clone()),
            Err(error) => log::error!("Failed to capture semantic Inspector history: {error}"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PropertyActionCommit {
    Deferred,
    Immediate,
}

fn action_commit_policy(action: &PropertyAction) -> PropertyActionCommit {
    match action {
        PropertyAction::ToggleKeyframe(..)
        | PropertyAction::SetAttribute(..)
        | PropertyAction::SetMode(..) => PropertyActionCommit::Immediate,
        PropertyAction::Update(..)
        | PropertyAction::UpdateGroup(..)
        | PropertyAction::Commit
        | PropertyAction::SetExpressionSource(..) => PropertyActionCommit::Deferred,
    }
}

fn default_open(group: SemanticPropertyGroup) -> bool {
    matches!(
        group,
        SemanticPropertyGroup::Container
            | SemanticPropertyGroup::Source
            | SemanticPropertyGroup::Transform
            | SemanticPropertyGroup::Style
    )
}

fn property_capabilities(
    access: &SemanticPropertyAccess,
    animation: SemanticAnimationSupport,
) -> (bool, bool) {
    let editable = matches!(access, SemanticPropertyAccess::Editable);
    let show_authoring = editable && animation == SemanticAnimationSupport::Evaluator;
    (editable, show_authoring)
}

fn render_root_error(ui: &mut Ui, clip_id: uuid::Uuid, message: &str) {
    let response = ui.colored_label(
        ui.visuals().error_fg_color,
        format!("Clip graph cannot be inspected safely: {message}"),
    );
    crate::qa::register_component_with_metadata(
        format!("inspector.semantic.clip:{clip_id}.error"),
        "inspector_semantic_diagnostic",
        response.rect,
        true,
        Some(serde_json::json!({
            "clip_id": clip_id,
            "severity": "error",
            "message": message,
            "fail_closed": true,
        })),
    );
}

fn render_stack_diagnostics(
    ui: &mut Ui,
    clip_id: uuid::Uuid,
    stack: &SemanticContainerPropertyStack,
) {
    for (index, message) in stack.diagnostics().iter().enumerate() {
        let response = ui.colored_label(ui.visuals().warn_fg_color, message);
        crate::qa::register_component_with_metadata(
            format!("inspector.semantic.clip:{clip_id}.diagnostic:{index}"),
            "inspector_semantic_diagnostic",
            response.rect,
            true,
            Some(serde_json::json!({
                "clip_id": clip_id,
                "severity": "warning",
                "message": message,
                "fail_closed": true,
            })),
        );
    }
}

fn render_section_diagnostics(ui: &mut Ui, clip_id: uuid::Uuid, section: &SemanticPropertySection) {
    for (index, message) in section.diagnostics().iter().enumerate() {
        let response = ui.colored_label(ui.visuals().warn_fg_color, message);
        crate::qa::register_component_with_metadata(
            format!(
                "inspector.semantic.section:{clip_id}:{}.diagnostic:{index}",
                section.stable_id()
            ),
            "inspector_semantic_diagnostic",
            response.rect,
            true,
            Some(serde_json::json!({
                "clip_id": clip_id,
                "section": section.stable_id(),
                "node_id": section.node_id(),
                "severity": "warning",
                "message": message,
                "fail_closed": true,
            })),
        );
    }
}

fn render_access_note(
    ui: &mut Ui,
    clip_id: uuid::Uuid,
    section: &SemanticPropertySection,
    key: &str,
    access: &SemanticPropertyAccess,
) {
    let message = match access {
        SemanticPropertyAccess::Editable => return,
        SemanticPropertyAccess::Wired { source } => {
            format!("Driven by {:?}.{}", source.owner, source.port)
        }
        SemanticPropertyAccess::ReadOnly { reason, .. } => reason.clone(),
    };
    let response = ui.add(
        egui::Label::new(RichText::new(&message).small().weak())
            .selectable(false)
            .wrap(),
    );
    crate::qa::register_component_with_metadata(
        format!(
            "inspector.semantic.property_access:{clip_id}:{}:{key}.note",
            section.stable_id()
        ),
        "inspector_semantic_property_diagnostic",
        response.rect,
        true,
        Some(serde_json::json!({
            "clip_id": clip_id,
            "section": section.stable_id(),
            "property": key,
            "message": message,
            "fail_closed": true,
        })),
    );
}

fn register_section(
    clip_id: uuid::Uuid,
    section: &SemanticPropertySection,
    rect: egui::Rect,
    open: bool,
) {
    crate::qa::register_component_with_metadata(
        format!(
            "inspector.semantic.section:{clip_id}:{}",
            section.stable_id()
        ),
        "inspector_semantic_section",
        rect,
        true,
        Some(serde_json::json!({
            "clip_id": clip_id,
            "stable_id": section.stable_id(),
            "label": section.label(),
            "group": format!("{:?}", section.group()).to_lowercase(),
            "owner": format!("{:?}", section.owner()),
            "node_id": section.node_id(),
            "property_count": section.properties().len(),
            "diagnostic_count": section.diagnostics().len(),
            "open": open,
        })),
    );
}

fn register_property_access(
    clip_id: uuid::Uuid,
    section: &SemanticPropertySection,
    key: &str,
    access: &SemanticPropertyAccess,
    rect: egui::Rect,
    editable: bool,
) {
    let (access_kind, source, reason, related_nodes) = match access {
        SemanticPropertyAccess::Editable => ("editable", None, None, Vec::new()),
        SemanticPropertyAccess::Wired { source } => ("wired", Some(source), None, Vec::new()),
        SemanticPropertyAccess::ReadOnly {
            reason,
            related_nodes,
        } => (
            "read_only",
            None,
            Some(reason.as_str()),
            related_nodes.clone(),
        ),
    };
    crate::qa::register_component_with_metadata(
        format!(
            "inspector.semantic.property_access:{clip_id}:{}:{key}",
            section.stable_id()
        ),
        "inspector_semantic_property",
        rect,
        editable,
        Some(serde_json::json!({
            "clip_id": clip_id,
            "section": section.stable_id(),
            "property": key,
            "access": access_kind,
            "source": source,
            "reason": reason,
            "related_nodes": related_nodes,
            "editable": editable,
            "fail_closed": !editable,
        })),
    );
}

fn render_action_error(
    ui: &mut Ui,
    clip_id: uuid::Uuid,
    section: &str,
    property: &str,
    message: &str,
) {
    let response = ui.colored_label(ui.visuals().error_fg_color, message);
    crate::qa::register_component_with_metadata(
        format!("inspector.semantic.action_error:{clip_id}:{section}:{property}"),
        "inspector_semantic_action_error",
        response.rect,
        true,
        Some(serde_json::json!({
            "clip_id": clip_id,
            "section": section,
            "property": property,
            "message": message,
            "history_committed": false,
            "fail_closed": true,
        })),
    );
}

#[cfg(test)]
mod tests;

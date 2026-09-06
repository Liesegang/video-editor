use super::*;

fn run_input(state: &mut NodeEditorState, events: Vec<egui::Event>) -> Vec<ModuleEditorAction> {
    let context = egui::Context::default();
    let mut actions = Vec::new();
    let _frame_output = context.run(input(events), |context| {
        egui::CentralPanel::default().show(context, |ui| {
            actions =
                keyboard_actions(ui, state, ui.max_rect(), egui::emath::TSTransform::IDENTITY);
        });
    });
    actions
}

fn input(events: Vec<egui::Event>) -> egui::RawInput {
    egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        )),
        events,
        ..Default::default()
    }
}

#[test]
fn clipboard_shortcuts_are_scoped_to_the_focused_dock_panel() {
    let id = Uuid::new_v4();
    let mut state = NodeEditorState {
        selected_nodes: HashSet::from([id]),
        ..Default::default()
    };
    assert!(run_input(&mut state, vec![egui::Event::Copy]).is_empty());
    state.keyboard_focused = true;
    assert_eq!(
        run_input(&mut state, vec![egui::Event::Copy]),
        vec![ModuleEditorAction::CopyNodes(vec![id])]
    );
}

#[test]
fn native_copy_and_raw_shortcut_produce_one_copy() {
    let id = Uuid::new_v4();
    let mut state = NodeEditorState {
        keyboard_focused: true,
        selected_nodes: HashSet::from([id]),
        ..Default::default()
    };
    let actions = run_input(
        &mut state,
        vec![
            egui::Event::Copy,
            egui::Event::Key {
                key: egui::Key::C,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::COMMAND,
            },
        ],
    );
    assert_eq!(actions, vec![ModuleEditorAction::CopyNodes(vec![id])]);
}

#[test]
fn a_native_paste_response_cannot_edit_a_different_host() {
    let old = ModuleEditorHost::NodeClip {
        timeline_item_id: library::model::authoring::TimelineItemId::new(),
        instance_path: None,
        module_instance_id: ModuleInstanceId::new(),
    };
    let new = ModuleEditorHost::NodeClip {
        timeline_item_id: library::model::authoring::TimelineItemId::new(),
        instance_path: None,
        module_instance_id: ModuleInstanceId::new(),
    };
    let mut state = NodeEditorState {
        pending_paste: Some((old.clone(), egui::pos2(120.0, 160.0), 10.0)),
        ..Default::default()
    };
    state.request_document(NodeEditorDocument::ModuleDefinition {
        definition_id: library::model::authoring::ModuleDefinitionId::new(),
        host: new,
    });
    assert!(run_input(&mut state, vec![egui::Event::Paste("ignored".into())]).is_empty());
    assert!(state.pending_paste.is_none());
    state.request_document(NodeEditorDocument::ModuleDefinition {
        definition_id: library::model::authoring::ModuleDefinitionId::new(),
        host: old.clone(),
    });
    state.pending_paste = Some((old, egui::pos2(120.0, 160.0), 10.0));
    assert_eq!(
        run_input(&mut state, vec![egui::Event::Paste("selection".into())]),
        vec![ModuleEditorAction::PasteNodes {
            text: "selection".into(),
            graph_position: egui::pos2(120.0, 160.0)
        }]
    );
}

#[test]
fn expired_native_response_cannot_become_a_fresh_focused_paste() {
    let mut state = NodeEditorState {
        keyboard_focused: true,
        pending_paste: Some((
            ModuleEditorHost::NodeClip {
                timeline_item_id: library::model::authoring::TimelineItemId::new(),
                instance_path: None,
                module_instance_id: ModuleInstanceId::new(),
            },
            egui::Pos2::ZERO,
            -1.0,
        )),
        ..Default::default()
    };
    assert!(run_input(&mut state, vec![egui::Event::Paste("text".into())]).is_empty());
    assert!(state.pending_paste.is_none());
}

#[test]
fn focused_text_edit_keeps_native_paste_from_the_node_clipboard_router() {
    let context = egui::Context::default();
    let mut state = NodeEditorState {
        keyboard_focused: true,
        selected_nodes: HashSet::from([Uuid::new_v4()]),
        ..Default::default()
    };
    let mut text = String::new();
    let mut actions = Vec::new();

    let _focus_frame = context.run(input(Vec::new()), |context| {
        egui::CentralPanel::default().show(context, |ui| {
            ui.text_edit_singleline(&mut text).request_focus();
        });
    });
    let _paste_frame = context.run(
        input(vec![egui::Event::Paste("typed text".into())]),
        |context| {
            egui::CentralPanel::default().show(context, |ui| {
                ui.text_edit_singleline(&mut text);
                actions = keyboard_actions(
                    ui,
                    &mut state,
                    ui.max_rect(),
                    egui::emath::TSTransform::IDENTITY,
                );
            });
        },
    );

    assert_eq!(text, "typed text");
    assert!(actions.is_empty());
    assert!(state.pending_paste.is_none());
}

use super::*;

struct Fixture {
    context: egui::Context,
    source: String,
    draft: String,
    committed: Vec<String>,
    error: Option<String>,
}

impl Fixture {
    fn new() -> Self {
        Self {
            context: egui::Context::default(),
            source: "heat".to_string(),
            draft: "heat".to_string(),
            committed: Vec::new(),
            error: None,
        }
    }

    fn frame(&mut self, events: Vec<egui::Event>, focus: bool) {
        let id = Id::new("test.name");
        drop(self.context.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 300.0),
                )),
                events,
                ..Default::default()
            },
            |context| {
                egui::CentralPanel::default().show(context, |ui| {
                    if focus {
                        ui.memory_mut(|memory| memory.request_focus(id));
                    }
                    let edit = name_editor(ui, id, &mut self.draft, &self.source, 220.0, |name| {
                        if name.is_empty() || name == "reserved" {
                            Err("Invalid name".to_string())
                        } else {
                            Ok(())
                        }
                    });
                    if let Some(value) = edit.value {
                        self.source.clone_from(&value);
                        self.committed.push(value);
                    }
                    self.error = edit.error;
                    ui.text_edit_singleline(&mut String::new());
                });
            },
        ));
    }
}

fn key(key: Key) -> egui::Event {
    egui::Event::Key {
        key,
        physical_key: Some(key),
        pressed: true,
        repeat: false,
        modifiers: egui::Modifiers::NONE,
    }
}

#[test]
fn typing_is_a_draft_until_enter_and_commits_once() {
    let mut fixture = Fixture::new();
    fixture.frame(Vec::new(), true);
    fixture.frame(vec![egui::Event::Text("x".to_string())], false);
    fixture.frame(vec![egui::Event::Text("y".to_string())], false);
    assert!(fixture.committed.is_empty());
    assert_eq!(fixture.source, "heat");
    let expected = fixture.draft.clone();
    assert_ne!(expected, fixture.source);
    fixture.frame(vec![key(Key::Enter)], false);
    fixture.frame(Vec::new(), false);
    assert_eq!(fixture.committed, vec![expected]);
}

#[test]
fn escape_restores_source_and_never_commits_on_lost_focus() {
    let mut fixture = Fixture::new();
    fixture.frame(Vec::new(), true);
    fixture.frame(vec![egui::Event::Text("draft".to_string())], false);
    assert_ne!(fixture.draft, fixture.source);
    fixture.frame(vec![key(Key::Escape)], false);
    fixture.frame(Vec::new(), false);
    assert_eq!(fixture.draft, "heat");
    assert!(fixture.committed.is_empty());
}

#[test]
fn lost_focus_commits_a_trimmed_name_but_rejects_invalid_names() {
    let mut fixture = Fixture::new();
    fixture.frame(Vec::new(), true);
    fixture.draft = "  hot  ".to_string();
    fixture.frame(vec![key(Key::Tab)], false);
    fixture.frame(Vec::new(), false);
    assert_eq!(fixture.committed, vec!["hot"]);
    for invalid in ["  ", "reserved"] {
        fixture.frame(Vec::new(), true);
        fixture.draft = invalid.to_string();
        fixture.frame(vec![key(Key::Enter)], false);
        fixture.frame(Vec::new(), false);
        assert!(fixture.error.is_some());
        assert_eq!(fixture.source, "hot");
        assert_eq!(fixture.committed, vec!["hot"]);
    }
}

#[test]
fn enter_in_another_control_does_not_commit_a_name_draft() {
    let mut fixture = Fixture::new();
    fixture.frame(Vec::new(), false);
    fixture.draft = "unfocused".to_string();
    fixture.frame(vec![key(Key::Enter)], false);
    assert!(fixture.committed.is_empty());
}

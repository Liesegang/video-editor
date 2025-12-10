use std::fs;
use std::io::Write;
use std::sync::{Arc, RwLock};
use eframe::egui::{self, Context, Visuals};
use egui_dock::{DockArea, DockState, Style};
use library::model::project::project::{Composition, Project};
use library::service::project_service::ProjectService;

use crate::action::HistoryManager;
use crate::model::ui_types::Tab;
use crate::state::context::EditorContext;
use crate::ui::tab_viewer::{AppTabViewer, create_initial_dock_state};
use crate::utils;


pub struct MyApp {
    pub editor_context: EditorContext,
    pub dock_state: DockState<Tab>,
    pub project_service: ProjectService,
    pub project: Arc<RwLock<Project>>,
    pub history_manager: HistoryManager,
}

impl MyApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Customize egui
        let mut visuals = Visuals::dark();
        visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(255, 120, 0);
        cc.egui_ctx.set_visuals(visuals);

        // Setup fonts
        utils::setup_fonts(&cc.egui_ctx);

        let default_project = Arc::new(RwLock::new(Project::new("Default Project")));
        // Add a default composition when the app starts
        let default_comp = Composition::new("Main Composition", 1920, 1080, 30.0, 60.0);
        let default_comp_id = default_comp.id;
        default_project.write().unwrap().add_composition(default_comp);

        let project_service = ProjectService::new(Arc::clone(&default_project));

        let mut editor_context = EditorContext::new(default_comp_id); // Pass default_comp_id
        editor_context.selected_composition_id = Some(default_comp_id); // Select the default composition

        let mut app = Self {
            editor_context,
            dock_state: create_initial_dock_state(),
            project_service,
            project: default_project,
            history_manager: HistoryManager::new(),
        };
        app.history_manager.push_project_state(app.project_service.get_project().read().unwrap().clone());
        cc.egui_ctx.request_repaint(); // Request repaint after initial state setup
        app
    }

    fn reset_layout(&mut self) {
        self.dock_state = create_initial_dock_state();
    }

    fn handle_shortcuts(&mut self, ctx: &Context) {
        ctx.input(|i| {
            if i.modifiers.command && i.key_pressed(egui::Key::Z) {
                let current_project = self.project_service.get_project().read().unwrap().clone();
                if i.modifiers.shift {
                    // Ctrl+Shift+Z for Redo
                    if let Some(new_project) = self.history_manager.redo(current_project) {
                        self.project_service.set_project(new_project);
                        self.editor_context.inspector_entity_cache = None; // Clear cache
                        ctx.request_repaint();
                    } else {
                        eprintln!("Redo stack is empty.");
                    }
                } else {
                    // Ctrl+Z for Undo
                    if let Some(new_project) = self.history_manager.undo(current_project) {
                        self.project_service.set_project(new_project);
                        self.editor_context.inspector_entity_cache = None; // Clear cache
                        ctx.request_repaint();
                    } else {
                        eprintln!("Undo stack is empty.");
                    }
                }
            }
            if i.key_pressed(egui::Key::Space) {
                self.editor_context.is_playing = !self.editor_context.is_playing;
            }
            if i.key_pressed(egui::Key::Delete) {
                if let Some(comp_id) = self.editor_context.selected_composition_id {
                    if let Some(track_id) = self.editor_context.selected_track_id {
                                                        if let Some(entity_id) = self.editor_context.selected_entity_id {
                                                            let prev_project_state = self.project_service.get_project().read().unwrap().clone();
                                                            if let Err(e) = self
                                                                .project_service
                                                                .remove_entity_from_track(comp_id, track_id, entity_id)
                                                            {
                                                                eprintln!("Failed to remove entity: {:?}", e);
                                                            } else {
                                                                self.editor_context.selected_entity_id = None;
                                                                self.history_manager.push_project_state(prev_project_state);
                                                            }                            // Request repaint if entity was removed
                            ctx.request_repaint();
                        }
                    }
                }
            }
        });
    }
}

impl eframe::App for MyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_shortcuts(ctx);

        // Manage inspector_entity_cache
        let current_selected_entity_id = self.editor_context.selected_entity_id;
        let current_selected_composition_id = self.editor_context.selected_composition_id;
        let current_selected_track_id = self.editor_context.selected_track_id;

        let mut should_update_cache = false;

        // Check if selected_entity_id changed or if cache is empty
        if let Some(selected_id) = current_selected_entity_id {
            if self.editor_context.inspector_entity_cache.is_none() || self.editor_context.inspector_entity_cache.as_ref().unwrap().0 != selected_id {
                should_update_cache = true;
            }
        } else {
            // No entity selected, clear cache
            if self.editor_context.inspector_entity_cache.is_some() {
                self.editor_context.inspector_entity_cache = None;
            }
        }

        if should_update_cache {
            // Populate cache with new selected entity's data
            if let (Some(entity_id), Some(comp_id), Some(track_id)) = (current_selected_entity_id, current_selected_composition_id, current_selected_track_id) {
                if let Ok(proj_read) = self.project.read() {
                    if let Some(comp) = proj_read.compositions.iter().find(|c| c.id == comp_id) {
                        if let Some(track) = comp.tracks.iter().find(|t| t.id == track_id) {
                            if let Some(entity) = track.entities.iter().find(|e| e.id == entity_id) {
                                self.editor_context.inspector_entity_cache = Some((
                                    entity.id,
                                    entity.entity_type.clone(),
                                    entity.properties.clone(),
                                    entity.start_time,
                                    entity.end_time,
                                ));
                            }
                        }
                    }
                }
            }
        }

        egui::TopBottomPanel::top("menu_bar").show(ctx, |ui| {
            egui::menu::bar(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("New Project").clicked() {
                        let prev_project_state = self.project_service.get_project().read().unwrap().clone();
                        let new_comp_id = self
                            .project_service
                            .add_composition("Main Composition", 1920, 1080, 30.0, 60.0)
                            .expect("Failed to add composition");
                        self.editor_context.selected_composition_id = Some(new_comp_id);
                        self.history_manager.push_project_state(prev_project_state);
                        ui.close_menu();
                    }
                    if ui.button("Load Project").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Project File", &["json"])
                            .pick_file()
                        {
                            match fs::read_to_string(&path) {
                                Ok(s) => {
                                    if let Err(e) = self.project_service.load_project(&s) {
                                        eprintln!("Failed to load project: {}", e);
                                    } else {
                                        // Clear history and push the loaded project as the initial state
                                        self.history_manager = HistoryManager::new();
                                        self.history_manager.push_project_state(self.project_service.get_project().read().unwrap().clone());
                                        println!("Project loaded from {}", path.display());
                                    }
                                }
                                Err(e) => eprintln!("Failed to read project file: {}", e),
                            }
                        }
                        ui.close_menu();
                    }
                    if ui.button("Save Project").clicked() {
                        if let Some(path) = rfd::FileDialog::new()
                            .add_filter("Project File", &["json"])
                            .set_file_name("project.json")
                            .save_file()
                        {
                            match self.project_service.save_project() {
                                Ok(json_str) => match fs::File::create(&path) {
                                    Ok(mut file) => {
                                        if let Err(e) = file.write_all(json_str.as_bytes()) {
                                            eprintln!("Failed to write project to file: {}", e);
                                        } else {
                                            println!("Project saved to {}", path.display());
                                        }
                                    }
                                    Err(e) => eprintln!("Failed to create file: {}", e),
                                },
                                Err(e) => eprintln!("Failed to save project: {}", e),
                            }
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("Edit", |ui| {
                    if ui.button("Undo").clicked() {
                        let current_project = self.project_service.get_project().read().unwrap().clone();
                        if let Some(new_project) = self.history_manager.undo(current_project) {
                            self.project_service.set_project(new_project);
                            self.editor_context.inspector_entity_cache = None; // Clear cache
                            ctx.request_repaint();
                        } else {
                            eprintln!("Undo stack is empty.");
                        }
                        ui.close_menu();
                    }
                    if ui.button("Redo").clicked() {
                        let current_project = self.project_service.get_project().read().unwrap().clone();
                        if let Some(new_project) = self.history_manager.redo(current_project) {
                            self.project_service.set_project(new_project);
                            self.editor_context.inspector_entity_cache = None; // Clear cache
                            ctx.request_repaint();
                        } else {
                            eprintln!("Redo stack is empty.");
                        }
                        ui.close_menu();
                    }
                    ui.separator();
                    if ui.button("Delete").clicked() {
                        if let Some(comp_id) = self.editor_context.selected_composition_id {
                            if let Some(track_id) = self.editor_context.selected_track_id {
                                if let Some(entity_id) = self.editor_context.selected_entity_id {
                                    if let Err(e) = self
                                        .project_service
                                        .remove_entity_from_track(comp_id, track_id, entity_id)
                                    {
                                        eprintln!("Failed to remove entity: {:?}", e);
                                    } else {
                                        self.editor_context.selected_entity_id = None;
                                    }
                                }
                            }
                        }
                        ui.close_menu();
                    }
                });

                ui.menu_button("View", |ui| {
                    if ui.button("Reset Layout").clicked() {
                        self.reset_layout();
                        ui.close_menu();
                    }
                });
            });
        });

        egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Ready");
                ui.separator();
                ui.label(format!("Time: {:.2}", self.editor_context.current_time));
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let mut tab_viewer = AppTabViewer::new(
                &mut self.editor_context,
                &mut self.history_manager,
                &mut self.project_service,
                &self.project, // Pass project here
            );
            DockArea::new(&mut self.dock_state)
                .style(Style::from_egui(ui.style().as_ref()))
                .show_inside(ui, &mut tab_viewer);

            if self.editor_context.is_playing {
                ui.ctx().request_repaint(); // Request repaint to update time
            }
        });

        if ctx.input(|i| i.pointer.any_released()) {
            self.editor_context.dragged_asset = None;
        }

        if self.editor_context.is_playing {
            self.editor_context.current_time += 0.016; // Assuming 60fps
        }
    }
}
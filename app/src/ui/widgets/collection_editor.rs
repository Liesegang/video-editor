use crate::ui::widgets::reorderable_list::ReorderableList;
use egui::{CollapsingHeader, Id, Response, Ui};
use std::hash::Hash;

pub struct CollectionEditor<'a, T> {
    id_salt: Id,
    items: &'a mut Vec<T>,
    header_name: String,
}

impl<'a, T> CollectionEditor<'a, T>
where
    T: Hash + Eq,
{
    pub fn new(id_salt: impl Hash, items: &'a mut Vec<T>, header_name: impl Into<String>) -> Self {
        Self {
            id_salt: Id::new(id_salt),
            items,
            header_name: header_name.into(),
        }
    }

    pub fn show<add_ui, item_ui>(
        self,
        ui: &mut Ui,
        mut add_ui_impl: add_ui,
        mut item_ui_impl: item_ui,
    ) where
        add_ui: FnMut(&mut Ui, &mut Vec<T>),
        item_ui: FnMut(&mut Ui, usize, &mut T) -> Option<bool>, // Returns Some(true) to delete
    {
        let id = self.id_salt;
        let header_name = self.header_name;

        CollapsingHeader::new(&header_name)
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    add_ui_impl(ui, self.items);
                });

                ui.add_space(4.0);

                let mut delete_index = None;
                ReorderableList::new(id, self.items).show(ui, |ui, index, item, handle| {
                    ui.horizontal(|ui| {
                        handle.ui(ui, |ui| {
                            ui.label("::");
                        });

                        // Render Item
                        // If item renderer returns Some(true), mark for deletion
                        if let Some(should_delete) = item_ui_impl(ui, index, item) {
                            if should_delete {
                                delete_index = Some(index);
                            }
                        }
                    });
                });

                if let Some(idx) = delete_index {
                    if idx < self.items.len() {
                        self.items.remove(idx);
                    }
                }
            });
    }
}

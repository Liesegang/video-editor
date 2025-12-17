use egui::{Id, Ui};
use egui_dnd::dnd;
use std::hash::Hash;

#[allow(dead_code)]
pub struct ReorderableList<'a, T> {
    id_source: Id,
    items: &'a mut Vec<T>,
}

impl<'a, T> ReorderableList<'a, T>
where
    T: Hash + Eq,
{
    pub fn new(id_source: impl Into<Id>, items: &'a mut Vec<T>) -> Self {
        Self {
            id_source: id_source.into(),
            items,
        }
    }

    pub fn show<F>(self, ui: &mut Ui, mut item_ui: F)
    where
        F: FnMut(&mut Ui, usize, &mut T, egui_dnd::Handle),
    {
        let response =
            dnd(ui, self.id_source).show(self.items.iter_mut(), |ui, item, handle, state| {
                item_ui(ui, state.index, item, handle);
            });

        response.update_vec(self.items);
    }
}

use egui::{Id, Ui};
use egui_dnd::dnd;
use std::hash::Hash;

pub struct CollectionEditor<'a, T, ItemUiFn, UpdateFn> {
    id_source: Id,
    items: &'a mut Vec<T>,
    render_item: ItemUiFn,
    on_update: UpdateFn,
}

impl<'a, T, ItemUiFn, UpdateFn> CollectionEditor<'a, T, ItemUiFn, UpdateFn>
where
    T: Hash + Eq + Clone + Send + Sync,
    ItemUiFn: FnMut(
        &mut Ui,
        usize,
        &mut T,
        egui_dnd::Handle,
        &mut crate::action::HistoryManager,
        &mut library::EditorService, // mutable
        &mut bool,
    ) -> bool, // returns true if should delete
    UpdateFn: FnOnce(Vec<T>, &mut library::EditorService) -> Result<(), library::LibraryError>,
{
    pub fn new(
        id_source: impl Into<Id>,
        items: &'a mut Vec<T>,
        render_item: ItemUiFn,
        on_update: UpdateFn,
    ) -> Self {
        Self {
            id_source: id_source.into(),
            items,
            render_item,
            on_update,
        }
    }

    pub fn show(
        self,
        ui: &mut Ui,
        history_manager: &mut crate::action::HistoryManager,
        project_service: &mut library::EditorService,
        needs_refresh: &mut bool,
    ) {
        let mut dnd_items = self.items.clone();
        let old_items = dnd_items.clone();
        let mut needs_delete_idx = None;

        let mut render_item = self.render_item;

        let response =
            dnd(ui, self.id_source).show(dnd_items.iter_mut(), |ui, item, handle, state| {
                let should_delete = (render_item)(
                    ui,
                    state.index,
                    item,
                    handle,
                    history_manager,
                    project_service,
                    needs_refresh,
                );
                if should_delete {
                    needs_delete_idx = Some(state.index);
                }
            });

        if response.final_update().is_some() {
            response.update_vec(&mut dnd_items);
        }

        if let Some(idx) = needs_delete_idx {
            if idx < dnd_items.len() {
                dnd_items.remove(idx);
            }
        }

        let changed = dnd_items.len() != old_items.len()
            || dnd_items.iter().zip(old_items.iter()).any(|(a, b)| a != b);

        if changed {
            if (self.on_update)(dnd_items.clone(), project_service).is_ok() {
                let current_state = project_service.get_project().read().unwrap().clone();
                history_manager.push_project_state(current_state);
                *self.items = dnd_items;
                *needs_refresh = true;
            }
        }
    }
}

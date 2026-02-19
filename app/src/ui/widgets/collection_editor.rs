use egui::{Id, Ui};
use egui_dnd::dnd;
use std::hash::{Hash, Hasher};

#[derive(Clone)]
struct DndItem<T> {
    item: T,
    id: Id,
}

impl<T> Hash for DndItem<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

impl<T> PartialEq for DndItem<T> {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<T> Eq for DndItem<T> {}

pub struct CollectionEditor<'a, T, ItemUiFn, UpdateFn, IdFn> {
    id_source: Id,
    items: &'a mut Vec<T>,
    get_id: IdFn,
    render_item: ItemUiFn,
    on_update: UpdateFn,
}

impl<'a, T, ItemUiFn, UpdateFn, IdFn> CollectionEditor<'a, T, ItemUiFn, UpdateFn, IdFn>
where
    T: Clone + Send + Sync,
    IdFn: Fn(&T) -> Id,
    ItemUiFn: FnMut(
        &mut Ui,
        usize,
        &mut T,
        egui_dnd::Handle,
        &mut crate::action::HistoryManager,
        &mut library::EditorService,
        &mut bool,
    ) -> bool,
    UpdateFn: FnOnce(Vec<T>, &mut library::EditorService) -> Result<(), library::LibraryError>,
{
    pub fn new(
        id_source: impl Into<Id>,
        items: &'a mut Vec<T>,
        get_id: IdFn,
        render_item: ItemUiFn,
        on_update: UpdateFn,
    ) -> Self {
        Self {
            id_source: id_source.into(),
            items,
            get_id,
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
        let mut dnd_items: Vec<DndItem<T>> = self
            .items
            .iter()
            .map(|item| DndItem {
                item: item.clone(),
                id: (self.get_id)(item),
            })
            .collect();
        let old_items = dnd_items.clone();
        let mut needs_delete_idx = None;

        let mut render_item = self.render_item;

        let response =
            dnd(ui, self.id_source).show(dnd_items.iter_mut(), |ui, dnd_item, handle, state| {
                let should_delete = (render_item)(
                    ui,
                    state.index,
                    &mut dnd_item.item, // Pass inner item
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
            let new_items: Vec<T> = dnd_items.into_iter().map(|wrapper| wrapper.item).collect();
            if (self.on_update)(new_items.clone(), project_service).is_ok() {
                let current_state = project_service.with_project(|p| p.clone());
                history_manager.push_project_state(current_state);
                *self.items = new_items;
                *needs_refresh = true;
            }
        }
    }
}

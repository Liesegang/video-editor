use super::types::{ContextMenu, MenuItem};

/// Builder for constructing context menus declaratively.
pub struct ContextMenuBuilder<A: Clone> {
    items: Vec<MenuItem<A>>,
}

impl<A: Clone> ContextMenuBuilder<A> {
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Add a clickable action item.
    pub fn action(mut self, label: impl Into<String>, action: A) -> Self {
        self.items.push(MenuItem::Action {
            label: label.into(),
            icon: None,
            action: Some(action),
            enabled: true,
            danger: false,
        });
        self
    }

    /// Add a clickable action with an icon.
    pub fn action_with_icon(
        mut self,
        icon: impl Into<String>,
        label: impl Into<String>,
        action: A,
    ) -> Self {
        self.items.push(MenuItem::Action {
            label: label.into(),
            icon: Some(icon.into()),
            action: Some(action),
            enabled: true,
            danger: false,
        });
        self
    }

    /// Add a destructive (danger) action, rendered in red.
    pub fn danger_action(
        mut self,
        icon: impl Into<String>,
        label: impl Into<String>,
        action: A,
    ) -> Self {
        self.items.push(MenuItem::Action {
            label: label.into(),
            icon: Some(icon.into()),
            action: Some(action),
            enabled: true,
            danger: true,
        });
        self
    }

    /// Add a disabled action (grayed out, not clickable).
    pub fn disabled(mut self, label: impl Into<String>) -> Self {
        self.items.push(MenuItem::Action {
            label: label.into(),
            icon: None,
            action: None,
            enabled: false,
            danger: false,
        });
        self
    }

    /// Add a visual separator.
    pub fn separator(mut self) -> Self {
        self.items.push(MenuItem::Separator);
        self
    }

    /// Add a non-interactive label/header.
    pub fn label(mut self, text: impl Into<String>) -> Self {
        self.items.push(MenuItem::Label(text.into()));
        self
    }

    /// Add a sub-menu.
    pub fn submenu(
        mut self,
        label: impl Into<String>,
        build: impl FnOnce(ContextMenuBuilder<A>) -> ContextMenuBuilder<A>,
    ) -> Self {
        let sub = build(ContextMenuBuilder::new());
        self.items.push(MenuItem::SubMenu {
            label: label.into(),
            icon: None,
            items: sub.items,
        });
        self
    }

    /// Build into a ContextMenu descriptor.
    pub fn build(self) -> ContextMenu<A> {
        ContextMenu { items: self.items }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_creates_empty_menu() {
        let menu: ContextMenu<i32> = ContextMenuBuilder::new().build();
        assert!(menu.items.is_empty());
    }

    #[test]
    fn builder_adds_single_action() {
        let menu = ContextMenuBuilder::new().action("Delete", 1).build();
        assert_eq!(menu.items.len(), 1);
        match &menu.items[0] {
            MenuItem::Action {
                label,
                enabled,
                danger,
                action,
                ..
            } => {
                assert_eq!(label, "Delete");
                assert!(*enabled);
                assert!(!*danger);
                assert_eq!(action.as_ref(), Some(&1));
            }
            _ => panic!("Expected Action variant"),
        }
    }

    #[test]
    fn builder_adds_separator() {
        let menu = ContextMenuBuilder::new()
            .action("A", 1)
            .separator()
            .action("B", 2)
            .build();
        assert_eq!(menu.items.len(), 3);
        assert!(matches!(menu.items[1], MenuItem::Separator));
    }

    #[test]
    fn builder_adds_label() {
        let menu = ContextMenuBuilder::new()
            .label("Header")
            .action("Item", 1)
            .build();
        assert!(matches!(&menu.items[0], MenuItem::Label(s) if s == "Header"));
    }

    #[test]
    fn builder_adds_action_with_icon() {
        let menu = ContextMenuBuilder::new()
            .action_with_icon("X", "Remove", 42)
            .build();
        match &menu.items[0] {
            MenuItem::Action { icon, label, .. } => {
                assert_eq!(icon.as_deref(), Some("X"));
                assert_eq!(label, "Remove");
            }
            _ => panic!("Expected Action"),
        }
    }

    #[test]
    fn builder_adds_danger_action() {
        let menu = ContextMenuBuilder::new()
            .danger_action("!", "Delete Forever", 99)
            .build();
        match &menu.items[0] {
            MenuItem::Action { danger, .. } => assert!(*danger),
            _ => panic!("Expected Action"),
        }
    }

    #[test]
    fn builder_adds_disabled_item() {
        let menu: ContextMenu<i32> = ContextMenuBuilder::new().disabled("Cannot do this").build();
        match &menu.items[0] {
            MenuItem::Action {
                enabled, action, ..
            } => {
                assert!(!*enabled);
                assert!(action.is_none());
            }
            _ => panic!("Expected disabled Action"),
        }
    }

    #[test]
    fn builder_adds_submenu() {
        let menu = ContextMenuBuilder::new()
            .submenu("More", |b| b.action("Sub A", 10).action("Sub B", 20))
            .build();
        match &menu.items[0] {
            MenuItem::SubMenu { label, items, .. } => {
                assert_eq!(label, "More");
                assert_eq!(items.len(), 2);
            }
            _ => panic!("Expected SubMenu"),
        }
    }

    #[test]
    fn builder_preserves_item_order() {
        let menu = ContextMenuBuilder::new()
            .action("First", 1)
            .separator()
            .action("Second", 2)
            .separator()
            .action("Third", 3)
            .build();
        assert_eq!(menu.items.len(), 5);
        assert!(matches!(&menu.items[0], MenuItem::Action { label, .. } if label == "First"));
        assert!(matches!(&menu.items[1], MenuItem::Separator));
        assert!(matches!(&menu.items[2], MenuItem::Action { label, .. } if label == "Second"));
        assert!(matches!(&menu.items[3], MenuItem::Separator));
        assert!(matches!(&menu.items[4], MenuItem::Action { label, .. } if label == "Third"));
    }
}

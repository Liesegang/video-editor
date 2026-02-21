/// A single item in a context menu.
#[derive(Clone, Debug)]
pub enum MenuItem<A: Clone> {
    /// A clickable action button.
    Action {
        label: String,
        icon: Option<String>,
        action: Option<A>,
        enabled: bool,
        danger: bool,
    },
    /// A visual separator line.
    Separator,
    /// A non-interactive label/header.
    Label(String),
    /// A nested sub-menu.
    SubMenu {
        label: String,
        icon: Option<String>,
        items: Vec<MenuItem<A>>,
    },
}

/// A fully described context menu, ready to render.
#[derive(Clone, Debug)]
pub struct ContextMenu<A: Clone> {
    pub items: Vec<MenuItem<A>>,
}

/// A menu item that can be categorized and searched.
#[derive(Clone, Debug)]
pub struct SearchableItem<A: Clone> {
    pub label: String,
    pub category: Option<String>,
    pub icon: Option<String>,
    pub action: A,
    pub enabled: bool,
    pub keywords: Vec<String>,
}

//! The application menus as a craft-menu tree.
//!
//! The same table of commands ([`crate::commands::COMMANDS`]) feeds the in-window bar and the
//! native macOS menu bar; this module builds the tree and maps a clicked [`CommandRef`] back to
//! what the app should do.

use craft_menu::{CommandRef, Document, Item, ItemRole, Menu, MenuBar, MenuRole, Node};

use crate::commands::{AppCommand, CommandState, Menu as AppMenu, menu_commands};
use crate::settings::WorkspaceShortcuts;
use crate::workspace::PanelId;

const WORKSPACE_ACTIVATE: &str = "workspace.activate";
const PANEL_TOGGLE: &str = "workspace.toggle_panel";
const WORKSPACE_SAVE_AS: &str = "workspace.save_as";
const WORKSPACE_RENAME: &str = "workspace.rename";
const WORKSPACE_RESET: &str = "workspace.reset";
const WORKSPACE_DELETE: &str = "workspace.delete";

/// What the menus show right now.
pub struct MenuInputs<'a> {
    pub shortcuts: &'a WorkspaceShortcuts,
    pub state: &'a CommandState,
    pub workspaces: Vec<String>,
    pub active_workspace: String,
    /// Every panel with whether the active workspace shows it.
    pub panels: Vec<(PanelId, bool)>,
    /// The label to show for English text: a command's label (its id, then the English label) or other menu text (an
    /// empty id). [`interface_label`] in the app; command ids and parameters stay English whatever it returns.
    pub label: &'a dyn Fn(&str, &str) -> String,
}

/// The labels a craft-menu layout writes itself (Exit, Preferences…, About {app}, …) in the interface language, for
/// [`craft_menu::LayoutOptions::label`].
pub fn layout_label(english: &'static str) -> String {
    crate::i18n::t(english).to_owned()
}

/// [`MenuInputs::label`] in the interface language: a command's label by its id, other text by itself.
pub fn interface_label(id: &str, english: &str) -> String {
    if id.is_empty() { crate::i18n::t(english).to_owned() } else { crate::i18n::tid(id, english).to_owned() }
}

/// What a clicked menu item asks the app to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuAction {
    Command(AppCommand),
    ActivateWorkspace(String),
    TogglePanel(PanelId),
}

fn menu_role(menu: AppMenu) -> MenuRole {
    match menu {
        AppMenu::File => MenuRole::File,
        AppMenu::Edit => MenuRole::Edit,
        AppMenu::Window => MenuRole::Window,
        AppMenu::Help => MenuRole::Help,
        AppMenu::Queue | AppMenu::Preset => MenuRole::Other,
    }
}

fn item_role(command: AppCommand) -> Option<ItemRole> {
    match command {
        AppCommand::Preferences => Some(ItemRole::Settings),
        AppCommand::About => Some(ItemRole::About),
        AppCommand::Quit => Some(ItemRole::Quit),
        _ => None,
    }
}

fn command_item(inputs: &MenuInputs<'_>, command: AppCommand) -> Option<Item> {
    let info = command.info()?;
    let english = info.label.replace("...", "…");
    let mut item = Item::new(CommandRef::new(info.id), (inputs.label)(info.id, &english)).english(english).enabled(command.enabled(inputs.state));
    if let Some(chord) = inputs.shortcuts.command_chord(command) {
        item = item.shortcut(chord);
    }
    if let Some(role) = item_role(command) {
        item = item.role(role);
    }
    Some(item)
}

fn window_extras(inputs: &MenuInputs<'_>) -> Vec<Node> {
    let panels = inputs
        .panels
        .iter()
        .map(|(panel, shown)| {
            let command = CommandRef::with_params(PANEL_TOGGLE, serde_json::json!({ "panel": panel.label() }));
            Node::Item(Item::new(command, (inputs.label)("", panel.label())).english(panel.label()).check(*shown))
        })
        .collect();
    let workspace = |id: &str, label: &str| Node::Item(Item::new(CommandRef::new(id), (inputs.label)("", label)).english(label));
    vec![
        Node::Separator,
        Node::submenu((inputs.label)("", "Panels"), panels).english("Panels"),
        Node::Separator,
        workspace(WORKSPACE_SAVE_AS, "Save Workspace As…"),
        workspace(WORKSPACE_RENAME, "Rename Workspace…"),
        workspace(WORKSPACE_RESET, "Reset Workspace"),
        workspace(WORKSPACE_DELETE, "Delete Workspace"),
    ]
}

/// The full menu tree for the current app state.
pub fn build_bar(inputs: &MenuInputs<'_>) -> MenuBar {
    let mut menus = Vec::new();
    for menu in AppMenu::ALL {
        let mut children = Vec::new();
        for info in menu_commands(menu) {
            let Some(item) = command_item(inputs, info.command) else { continue };
            if info.separator_before && !children.is_empty() {
                children.push(Node::Separator);
            }
            children.push(Node::Item(item));
        }
        if menu == AppMenu::Window {
            children.extend(window_extras(inputs));
        }
        menus.push(Menu::new((inputs.label)("", menu.label()), children).english(menu.label()).role(menu_role(menu)));
    }
    let documents = inputs
        .workspaces
        .iter()
        .map(|name| {
            let command = CommandRef::with_params(WORKSPACE_ACTIVATE, serde_json::json!({ "name": name }));
            Document::new(command, name.clone()).active(*name == inputs.active_workspace)
        })
        .collect();
    MenuBar::new(menus).documents(documents)
}

/// Maps a clicked item back to the app's action.
pub fn resolve(command: &CommandRef) -> Option<MenuAction> {
    match command.id.as_str() {
        WORKSPACE_ACTIVATE => {
            let name = command.params.get("name")?.as_str()?;
            Some(MenuAction::ActivateWorkspace(name.to_string()))
        }
        PANEL_TOGGLE => {
            let label = command.params.get("panel")?.as_str()?;
            PanelId::ALL.into_iter().find(|p| p.label() == label).map(MenuAction::TogglePanel)
        }
        WORKSPACE_SAVE_AS => Some(MenuAction::Command(AppCommand::SaveWorkspaceAs)),
        WORKSPACE_RENAME => Some(MenuAction::Command(AppCommand::RenameWorkspace)),
        WORKSPACE_RESET => Some(MenuAction::Command(AppCommand::ResetWorkspace)),
        WORKSPACE_DELETE => Some(MenuAction::Command(AppCommand::DeleteWorkspace)),
        id => crate::commands::COMMANDS.iter().find(|c| c.id == id).map(|c| MenuAction::Command(c.command)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs<'a>(shortcuts: &'a WorkspaceShortcuts, state: &'a CommandState) -> MenuInputs<'a> {
        MenuInputs {
            shortcuts,
            state,
            workspaces: vec!["Default".into(), "Review".into()],
            active_workspace: "Review".into(),
            panels: PanelId::ALL.into_iter().map(|p| (p, true)).collect(),
            label: &|_, english| english.to_owned(),
        }
    }

    #[test]
    fn every_visible_command_is_in_the_tree_and_resolves() {
        let shortcuts = WorkspaceShortcuts::default();
        let state = CommandState::default();
        let bar = build_bar(&inputs(&shortcuts, &state));
        for menu in AppMenu::ALL {
            for info in menu_commands(menu) {
                let found = bar.find(&CommandRef::new(info.id));
                assert!(found.is_some(), "{} missing from the menus", info.id);
                assert_eq!(resolve(&CommandRef::new(info.id)), Some(MenuAction::Command(info.command)));
            }
        }
    }

    #[test]
    fn system_items_carry_their_roles() {
        let shortcuts = WorkspaceShortcuts::default();
        let state = CommandState::default();
        let bar = build_bar(&inputs(&shortcuts, &state));
        for (id, role) in [("edit.preferences", ItemRole::Settings), ("help.about", ItemRole::About), ("file.quit", ItemRole::Quit)] {
            assert_eq!(bar.find(&CommandRef::new(id)).and_then(|i| i.role), Some(role), "{id}");
        }
    }

    #[test]
    fn workspaces_are_documents_and_the_active_one_resolves() {
        let shortcuts = WorkspaceShortcuts::default();
        let state = CommandState::default();
        let bar = build_bar(&inputs(&shortcuts, &state));
        assert_eq!(bar.documents.len(), 2);
        let review = bar.documents.iter().find(|d| d.label == "Review");
        assert!(review.is_some_and(|d| d.active));
        let command = CommandRef::with_params(WORKSPACE_ACTIVATE, serde_json::json!({ "name": "Review" }));
        assert_eq!(resolve(&command), Some(MenuAction::ActivateWorkspace("Review".into())));
    }

    #[test]
    fn panel_toggles_resolve_and_unknown_ids_do_not() {
        let toggle = CommandRef::with_params(PANEL_TOGGLE, serde_json::json!({ "panel": "Queue" }));
        assert_eq!(resolve(&toggle), Some(MenuAction::TogglePanel(PanelId::Queue)));
        assert_eq!(resolve(&CommandRef::new("nope")), None);
        let bad = CommandRef::with_params(PANEL_TOGGLE, serde_json::json!({ "panel": 3 }));
        assert_eq!(resolve(&bad), None);
    }

    #[test]
    fn translated_labels_keep_english_ids_parameters_and_mnemonic_labels() {
        let shortcuts = WorkspaceShortcuts::default();
        let state = CommandState::default();
        let translate = |_: &str, english: &str| format!("«{english}»");
        let bar = build_bar(&MenuInputs { label: &translate, ..inputs(&shortcuts, &state) });
        for menu in &bar.menus {
            assert!(menu.title.starts_with('«'), "menu title {} is translated", menu.title);
            assert!(menu.english.as_deref().is_some_and(|english| !english.starts_with('«')), "{} keeps its English title", menu.title);
        }
        for menu in AppMenu::ALL {
            for info in menu_commands(menu) {
                let item = bar.find(&CommandRef::new(info.id));
                assert!(item.is_some_and(|item| item.label.starts_with('«')), "{} shows a translated label", info.id);
                assert_eq!(resolve(&CommandRef::new(info.id)), Some(MenuAction::Command(info.command)), "{} still resolves", info.id);
            }
        }
        let toggle = CommandRef::with_params(PANEL_TOGGLE, serde_json::json!({ "panel": PanelId::Queue.label() }));
        assert!(bar.find(&toggle).is_some(), "panel toggles keep the English panel name as their parameter");
        assert_eq!(resolve(&toggle), Some(MenuAction::TogglePanel(PanelId::Queue)));
    }

    #[test]
    fn quit_has_a_command_shortcut() {
        let shortcuts = WorkspaceShortcuts::default();
        let state = CommandState::default();
        let bar = build_bar(&inputs(&shortcuts, &state));
        assert!(bar.find(&CommandRef::new("file.quit")).is_some_and(|i| i.shortcut.is_some()));
    }
}

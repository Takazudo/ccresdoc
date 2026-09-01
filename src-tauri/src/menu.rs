use std::collections::BTreeMap;

use tauri::menu::{Menu, MenuBuilder, MenuItem, MenuItemBuilder, Submenu, SubmenuBuilder};
use tauri::{Manager, Runtime, Wry};

use crate::settings_window::{SETTINGS_ACCELERATOR, SETTINGS_MENU_ID};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AppMenuEntry {
    About,
    Separator,
    Settings,
    Services,
    Hide,
    HideOthers,
    ShowAll,
    Quit,
}

/// macOS binds ⌘H to the app menu's Hide item — an app menu without
/// `Hide` silently loses ⌘H entirely. This slice is the source of truth
/// for the app submenu and is asserted by the test below.
const APP_MENU_SPEC: &[AppMenuEntry] = &[
    AppMenuEntry::About,
    AppMenuEntry::Separator,
    AppMenuEntry::Settings,
    AppMenuEntry::Separator,
    AppMenuEntry::Services,
    AppMenuEntry::Separator,
    AppMenuEntry::Hide,
    AppMenuEntry::HideOthers,
    AppMenuEntry::ShowAll,
    AppMenuEntry::Separator,
    AppMenuEntry::Quit,
];

fn build_app_submenu<R: Runtime, M: Manager<R>>(app: &M) -> tauri::Result<Submenu<R>> {
    let settings = MenuItemBuilder::with_id(SETTINGS_MENU_ID, "Settings…")
        .accelerator(SETTINGS_ACCELERATOR)
        .build(app)?;

    APP_MENU_SPEC
        .iter()
        .fold(
            SubmenuBuilder::new(app, "CCResDoc"),
            |builder, entry| match entry {
                AppMenuEntry::About => builder.about(None),
                AppMenuEntry::Separator => builder.separator(),
                AppMenuEntry::Settings => builder.item(&settings),
                AppMenuEntry::Services => builder.services(),
                AppMenuEntry::Hide => builder.hide(),
                AppMenuEntry::HideOthers => builder.hide_others(),
                AppMenuEntry::ShowAll => builder.show_all(),
                AppMenuEntry::Quit => builder.quit(),
            },
        )
        .build()
}

pub const BROWSER_MENU_ID_PREFIX: &str = "browser-command:";
#[cfg(test)]
const BROWSER_COMMAND_IDS: &[&str] = &[
    "back",
    "forward",
    "home",
    "reload-documentation",
    "find-in-page",
    "search-documentation",
    "copy-page-path",
    "open-in-default-browser",
];

#[derive(Clone)]
pub struct BrowserMenuHandles {
    items: BTreeMap<String, MenuItem<Wry>>,
}

impl BrowserMenuHandles {
    pub fn iter(&self) -> impl Iterator<Item = (&str, &MenuItem<Wry>)> {
        self.items
            .iter()
            .map(|(command_id, item)| (command_id.as_str(), item))
    }
}

pub struct BuiltMenu {
    pub menu: Menu<Wry>,
    pub browser: BrowserMenuHandles,
}

fn browser_item<M: Manager<Wry>>(
    app: &M,
    command_id: &str,
    label: &str,
) -> tauri::Result<MenuItem<Wry>> {
    MenuItemBuilder::with_id(format!("{BROWSER_MENU_ID_PREFIX}{command_id}"), label)
        .enabled(false)
        .build(app)
}

pub fn browser_command_id(menu_id: &str) -> Option<&str> {
    menu_id.strip_prefix(BROWSER_MENU_ID_PREFIX)
}

pub fn build<M: Manager<Wry>>(app: &M) -> tauri::Result<BuiltMenu> {
    let app_menu = build_app_submenu(app)?;

    let back = browser_item(app, "back", "Back")?;
    let forward = browser_item(app, "forward", "Forward")?;
    let home = browser_item(app, "home", "Home")?;
    let reload = browser_item(app, "reload-documentation", "Reload Documentation")?;
    let find = browser_item(app, "find-in-page", "Find in Page")?;
    let search = browser_item(app, "search-documentation", "Search Documentation")?;
    let copy_path = browser_item(app, "copy-page-path", "Copy Page Path")?;
    let open_browser = browser_item(app, "open-in-default-browser", "Open in Default Browser")?;

    let file_menu = SubmenuBuilder::new(app, "File")
        .item(&copy_path)
        .item(&open_browser)
        .build()?;

    let history_menu = SubmenuBuilder::new(app, "History")
        .item(&back)
        .item(&forward)
        .separator()
        .item(&home)
        .build()?;

    let edit_menu = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .separator()
        .item(&find)
        .item(&search)
        .build()?;

    let view_menu = SubmenuBuilder::new(app, "View")
        .item(&reload)
        .item(
            &MenuItemBuilder::with_id("devtools", "Toggle Developer Tools")
                .accelerator("CmdOrCtrl+Alt+I")
                .build(app)?,
        )
        .separator()
        .item(
            &MenuItemBuilder::with_id("actual_size", "Actual Size")
                .accelerator("CmdOrCtrl+0")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("zoom_in", "Zoom In")
                .accelerator("CmdOrCtrl+=")
                .build(app)?,
        )
        .item(
            &MenuItemBuilder::with_id("zoom_out", "Zoom Out")
                .accelerator("CmdOrCtrl+-")
                .build(app)?,
        )
        .separator()
        .fullscreen()
        .build()?;

    let window_menu = SubmenuBuilder::with_id(app, tauri::menu::WINDOW_SUBMENU_ID, "Window")
        .minimize()
        .maximize()
        .build()?;

    let menu = MenuBuilder::new(app)
        .item(&app_menu)
        .item(&file_menu)
        .item(&edit_menu)
        .item(&view_menu)
        .item(&history_menu)
        .item(&window_menu)
        .build()?;
    let items = [
        ("back", back),
        ("forward", forward),
        ("home", home),
        ("reload-documentation", reload),
        ("find-in-page", find),
        ("search-documentation", search),
        ("copy-page-path", copy_path),
        ("open-in-default-browser", open_browser),
    ]
    .into_iter()
    .map(|(command_id, item)| (command_id.into(), item))
    .collect();
    Ok(BuiltMenu {
        menu,
        browser: BrowserMenuHandles { items },
    })
}

#[cfg(test)]
mod tests {
    use super::{browser_command_id, AppMenuEntry::*, APP_MENU_SPEC, BROWSER_COMMAND_IDS};

    #[test]
    fn app_menu_spec_is_the_standard_macos_layout() {
        assert_eq!(
            APP_MENU_SPEC,
            &[
                About, Separator, Settings, Separator, Services, Separator, Hide, HideOthers,
                ShowAll, Separator, Quit,
            ],
        );
    }

    #[test]
    fn browser_menu_ids_are_namespaced_and_reversible() {
        assert_eq!(browser_command_id("browser-command:back"), Some("back"));
        assert_eq!(browser_command_id("refresh"), None);
    }

    #[test]
    fn every_catalog_command_has_one_retained_native_menu_item() {
        let catalog = crate::settings::browser_command_catalog();
        let catalog_ids = catalog
            .commands
            .iter()
            .map(|command| command.command_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(catalog_ids, BROWSER_COMMAND_IDS);
    }
}

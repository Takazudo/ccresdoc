use tauri::menu::{Menu, MenuBuilder, MenuItemBuilder, Submenu, SubmenuBuilder};
use tauri::{Manager, Runtime};

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

pub fn build<R: Runtime, M: Manager<R>>(app: &M) -> tauri::Result<Menu<R>> {
    let app_menu = build_app_submenu(app)?;

    let edit_menu = SubmenuBuilder::new(app, "Edit")
        .undo()
        .redo()
        .separator()
        .cut()
        .copy()
        .paste()
        .select_all()
        .build()?;

    let view_menu = SubmenuBuilder::new(app, "View")
        .item(
            &MenuItemBuilder::with_id("refresh", "Refresh")
                .accelerator("CmdOrCtrl+R")
                .build(app)?,
        )
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

    MenuBuilder::new(app)
        .item(&app_menu)
        .item(&edit_menu)
        .item(&view_menu)
        .item(&window_menu)
        .build()
}

#[cfg(test)]
mod tests {
    use super::{AppMenuEntry::*, APP_MENU_SPEC};

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
}

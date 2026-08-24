use crate::{appearance, AppState};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

pub const SETTINGS_WINDOW_LABEL: &str = "settings";
pub const SETTINGS_MENU_ID: &str = "open_settings";
pub const SETTINGS_ACCELERATOR: &str = "CmdOrCtrl+,";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleAction {
    HideSettings,
    StopForMainClose,
    Shutdown,
    ReopenMain,
    Ignore,
}

pub fn lifecycle_action(event: &str, label: Option<&str>) -> LifecycleAction {
    match (event, label) {
        ("close_requested", Some(SETTINGS_WINDOW_LABEL)) => LifecycleAction::HideSettings,
        ("destroyed", Some("main")) => LifecycleAction::StopForMainClose,
        ("exit_requested" | "exit", _) => LifecycleAction::Shutdown,
        ("reopen", _) => LifecycleAction::ReopenMain,
        _ => LifecycleAction::Ignore,
    }
}

trait WindowBoundary {
    type Error;

    fn exists(&self) -> bool;
    fn create(&mut self) -> Result<(), Self::Error>;
    fn show(&mut self) -> Result<(), Self::Error>;
    fn is_minimized(&self) -> Result<bool, Self::Error>;
    fn unminimize(&mut self) -> Result<(), Self::Error>;
    fn focus(&mut self) -> Result<(), Self::Error>;
}

fn drive_open_or_focus<B: WindowBoundary>(boundary: &mut B) -> Result<(), B::Error> {
    if !boundary.exists() {
        boundary.create()?;
    }
    boundary.show()?;
    if boundary.is_minimized()? {
        boundary.unminimize()?;
    }
    boundary.focus()
}

struct TauriWindowBoundary<'a> {
    app: &'a AppHandle,
}

impl TauriWindowBoundary<'_> {
    fn window(&self) -> Result<tauri::WebviewWindow, tauri::Error> {
        self.app
            .get_webview_window(SETTINGS_WINDOW_LABEL)
            .ok_or(tauri::Error::WindowNotFound)
    }
}

impl WindowBoundary for TauriWindowBoundary<'_> {
    type Error = tauri::Error;

    fn exists(&self) -> bool {
        self.app.get_webview_window(SETTINGS_WINDOW_LABEL).is_some()
    }

    fn create(&mut self) -> Result<(), Self::Error> {
        let state = self.app.state::<AppState>();
        let appearance = appearance::value_from_snapshot(&state.settings_store.load());
        let mut builder = WebviewWindowBuilder::new(
            self.app,
            SETTINGS_WINDOW_LABEL,
            WebviewUrl::App("settings.html".into()),
        )
        .title("CCResDoc Settings")
        .initialization_script(appearance::bundled_initialization_script(&appearance))
        .inner_size(720.0, 560.0)
        .min_inner_size(520.0, 420.0)
        .on_navigation(|url| {
            matches!(url.scheme(), "tauri" | "asset") || url.as_str() == "about:blank"
        });
        if crate::ephemeral_webview_enabled() {
            builder = builder.incognito(true);
        }
        builder.build()?;
        Ok(())
    }

    fn show(&mut self) -> Result<(), Self::Error> {
        self.window()?.show()
    }

    fn is_minimized(&self) -> Result<bool, Self::Error> {
        self.window()?.is_minimized()
    }

    fn unminimize(&mut self) -> Result<(), Self::Error> {
        self.window()?.unminimize()
    }

    fn focus(&mut self) -> Result<(), Self::Error> {
        self.window()?.set_focus()
    }
}

pub fn open_or_focus_settings(app: &AppHandle) -> Result<(), tauri::Error> {
    drive_open_or_focus(&mut TauriWindowBoundary { app })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeWindow {
        exists: bool,
        minimized: bool,
        actions: Vec<&'static str>,
    }

    impl WindowBoundary for FakeWindow {
        type Error = ();

        fn exists(&self) -> bool {
            self.exists
        }
        fn create(&mut self) -> Result<(), Self::Error> {
            self.actions.push("create");
            self.exists = true;
            Ok(())
        }
        fn show(&mut self) -> Result<(), Self::Error> {
            self.actions.push("show");
            Ok(())
        }
        fn is_minimized(&self) -> Result<bool, Self::Error> {
            Ok(self.minimized)
        }
        fn unminimize(&mut self) -> Result<(), Self::Error> {
            self.actions.push("unminimize");
            self.minimized = false;
            Ok(())
        }
        fn focus(&mut self) -> Result<(), Self::Error> {
            self.actions.push("focus");
            Ok(())
        }
    }

    #[test]
    fn first_open_creates_shows_and_focuses_once() {
        let mut window = FakeWindow::default();
        drive_open_or_focus(&mut window).unwrap();
        assert_eq!(window.actions, ["create", "show", "focus"]);
    }

    #[test]
    fn later_open_reuses_shows_unminimizes_and_focuses() {
        let mut window = FakeWindow {
            exists: true,
            minimized: true,
            ..Default::default()
        };
        drive_open_or_focus(&mut window).unwrap();
        assert_eq!(window.actions, ["show", "unminimize", "focus"]);
    }

    #[test]
    fn lifecycle_never_treats_settings_as_an_exit_event() {
        assert_eq!(
            lifecycle_action("close_requested", Some(SETTINGS_WINDOW_LABEL)),
            LifecycleAction::HideSettings
        );
        assert_eq!(
            lifecycle_action("destroyed", Some(SETTINGS_WINDOW_LABEL)),
            LifecycleAction::Ignore
        );
        assert_eq!(
            lifecycle_action("destroyed", Some("main")),
            LifecycleAction::StopForMainClose
        );
        assert_eq!(lifecycle_action("exit", None), LifecycleAction::Shutdown);
        assert_eq!(
            lifecycle_action("reopen", None),
            LifecycleAction::ReopenMain
        );
    }
}

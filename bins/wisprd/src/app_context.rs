use serde::Deserialize;
use wispr_core::models::{ActiveAppClass, ActiveAppContext};
use zbus::Proxy;

pub async fn detect_active_app() -> Option<ActiveAppContext> {
    let connection = zbus::Connection::session().await.ok()?;
    let proxy = Proxy::new(
        &connection,
        "org.gnome.Shell",
        "/org/gnome/Shell",
        "org.gnome.Shell",
    )
    .await
    .ok()?;

    let script = r#"
        const win = global.display.get_focus_window();
        if (!win) {
            '';
        } else {
            JSON.stringify({
                wm_class: win.get_wm_class_instance() || win.get_wm_class() || '',
                title: win.get_title() || ''
            });
        }
    "#;

    let (success, payload): (bool, String) = proxy.call("Eval", &(script)).await.ok()?;
    if !success || payload.trim().is_empty() {
        return None;
    }

    let info = serde_json::from_str::<ShellWindowInfo>(&payload).ok()?;
    let app_id = normalize_app_id(&info.wm_class);
    Some(ActiveAppContext {
        app_class: classify_app(&app_id),
        app_id: Some(app_id),
    })
}

#[derive(Deserialize)]
struct ShellWindowInfo {
    #[serde(default)]
    wm_class: String,
}

fn normalize_app_id(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

fn classify_app(app_id: &str) -> ActiveAppClass {
    if matches_browser(app_id) {
        ActiveAppClass::Browser
    } else if matches_editor(app_id) {
        ActiveAppClass::Editor
    } else if matches_terminal(app_id) {
        ActiveAppClass::Terminal
    } else {
        ActiveAppClass::Generic
    }
}

fn matches_browser(app_id: &str) -> bool {
    [
        "firefox",
        "google-chrome",
        "chrome",
        "chromium",
        "brave",
        "microsoft-edge",
        "edge",
        "vivaldi",
        "zen",
    ]
    .iter()
    .any(|candidate| app_id.contains(candidate))
}

fn matches_editor(app_id: &str) -> bool {
    [
        "code",
        "codium",
        "cursor",
        "gedit",
        "text-editor",
        "kate",
        "sublime",
        "jetbrains",
    ]
    .iter()
    .any(|candidate| app_id.contains(candidate))
}

fn matches_terminal(app_id: &str) -> bool {
    [
        "gnome-terminal",
        "ptyxis",
        "wezterm",
        "alacritty",
        "kitty",
        "xterm",
        "konsole",
        "tilix",
    ]
    .iter()
    .any(|candidate| app_id.contains(candidate))
}

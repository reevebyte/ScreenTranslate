#[cfg(target_os = "linux")]
use crate::state::{AppState, lock};
use crate::windows;
use screentrans_core::{AppError, ErrorCode, Result, config::Hotkeys};
#[cfg(target_os = "linux")]
use std::process::Command;
use tauri::AppHandle;
#[cfg(target_os = "linux")]
use tauri::{Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

pub fn dispatch(app: &AppHandle, action: &str) {
    match action {
        "capture" => windows::begin_capture(app.clone(), None),
        "quick" => {
            let _ = windows::toggle_quick(app);
        }
        "toggle" => {
            let _ = windows::toggle_result(app);
        }
        _ => {}
    }
}

pub fn apply(app: &AppHandle, keys: &Hotkeys) -> Result<()> {
    if cfg!(feature = "e2e") {
        return Ok(());
    }
    if screentrans_platform::is_wayland() {
        #[cfg(target_os = "linux")]
        {
            if is_gnome_desktop() {
                let bindings = gnome_bindings(keys)?;
                if sync_gnome_shortcuts(&bindings) {
                    mark_global_shortcuts(app);
                }
            }
        }
        return Ok(());
    }
    let entries = [
        ("capture", &keys.capture),
        ("toggle", &keys.toggle),
        ("quick", &keys.quick),
    ];
    let parsed: Vec<_> = entries
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
        .map(|(action, value)| {
            value
                .parse::<Shortcut>()
                .map(|key| (action, key))
                .map_err(|_| {
                    AppError::new(ErrorCode::Configuration, format!("快捷键格式无效：{value}"))
                        .action("hotkeys")
                })
        })
        .collect::<Result<_>>()?;
    app.global_shortcut()
        .unregister_all()
        .map_err(|_| shortcut_error())?;
    for (action, shortcut) in parsed {
        app.global_shortcut()
            .on_shortcut(shortcut, move |app, _, event| {
                if event.state == ShortcutState::Pressed {
                    dispatch(app, action);
                }
            })
            .map_err(|_| shortcut_error())?;
    }
    Ok(())
}

#[cfg(target_os = "linux")]
struct GnomeBinding {
    action: &'static str,
    name: &'static str,
    binding: String,
}

#[cfg(target_os = "linux")]
fn gnome_bindings(keys: &Hotkeys) -> Result<Vec<GnomeBinding>> {
    [
        ("capture", "ScreenTranslate 框选翻译", &keys.capture),
        ("toggle", "ScreenTranslate 收起显示", &keys.toggle),
        ("quick", "ScreenTranslate 快速文字翻译", &keys.quick),
    ]
    .into_iter()
    .filter(|(_, _, value)| !value.is_empty())
    .map(|(action, name, value)| {
        gnome_binding(value)
            .map(|binding| GnomeBinding {
                action,
                name,
                binding,
            })
            .ok_or_else(|| {
                AppError::new(ErrorCode::Configuration, format!("快捷键格式无效：{value}"))
                    .action("hotkeys")
            })
    })
    .collect()
}

#[cfg(target_os = "linux")]
fn gnome_binding(value: &str) -> Option<String> {
    let mut binding = String::new();
    for raw in value.split('+') {
        let part = raw.trim();
        if part.is_empty() {
            return None;
        }
        let lower = part.to_ascii_lowercase();
        let upper = part.to_ascii_uppercase();
        let function_key = upper.len() > 1
            && upper.starts_with('F')
            && upper[1..].chars().all(|c| c.is_ascii_digit());
        let token = match lower.as_str() {
            "ctrl" | "control" => "<Control>".to_string(),
            "alt" => "<Alt>".to_string(),
            "shift" => "<Shift>".to_string(),
            "super" | "meta" | "win" => "<Super>".to_string(),
            "space" => "space".to_string(),
            "enter" | "return" => "Return".to_string(),
            "tab" => "Tab".to_string(),
            "escape" | "esc" => "Escape".to_string(),
            "backspace" => "BackSpace".to_string(),
            "delete" => "Delete".to_string(),
            "arrowup" => "Up".to_string(),
            "arrowdown" => "Down".to_string(),
            "arrowleft" => "Left".to_string(),
            "arrowright" => "Right".to_string(),
            "pageup" => "Page_Up".to_string(),
            "pagedown" => "Page_Down".to_string(),
            "home" => "Home".to_string(),
            "end" => "End".to_string(),
            "insert" => "Insert".to_string(),
            _ if part.len() == 1 && part.chars().all(|c| c.is_ascii_alphanumeric()) => {
                part.to_ascii_lowercase()
            }
            _ if function_key => upper,
            _ => return None,
        };
        binding.push_str(&token);
    }
    (!binding.is_empty()).then_some(binding)
}

#[cfg(target_os = "linux")]
fn sync_gnome_shortcuts(bindings: &[GnomeBinding]) -> bool {
    if !is_gnome_desktop() {
        return false;
    }
    let executable = std::env::var_os("APPIMAGE")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::current_exe().ok());
    let Some(executable) = executable else {
        return false;
    };
    let executable = shell_quote(&executable.to_string_lossy());
    const MEDIA_KEYS: &str = "org.gnome.settings-daemon.plugins.media-keys";
    const SLOT_ROOT: &str = "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/";
    let slots = ["capture", "toggle", "quick"]
        .map(|action| format!("{SLOT_ROOT}screentranslate-{action}/"));
    let Some(mut paths) = gsettings_strings(MEDIA_KEYS, "custom-keybindings") else {
        return false;
    };
    paths.retain(|path| !slots.contains(path));

    for binding in bindings {
        let slot = format!("{SLOT_ROOT}screentranslate-{}/", binding.action);
        let schema =
            format!("org.gnome.settings-daemon.plugins.media-keys.custom-keybinding:{slot}");
        let argument = match binding.action {
            "capture" => "--capture",
            "toggle" => "--toggle-result",
            _ => "--quick",
        };
        let command = format!("{executable} {argument}");
        if !gsettings_set(&schema, "name", binding.name)
            || !gsettings_set(&schema, "command", &command)
            || !gsettings_set(&schema, "binding", &binding.binding)
        {
            return false;
        }
        paths.push(slot);
    }

    let array = format!(
        "[{}]",
        paths
            .iter()
            .map(|path| gvariant_string(path))
            .collect::<Vec<_>>()
            .join(", ")
    );
    gsettings_set(MEDIA_KEYS, "custom-keybindings", &array)
}

#[cfg(target_os = "linux")]
fn is_gnome_desktop() -> bool {
    let desktop = [
        "XDG_CURRENT_DESKTOP",
        "XDG_SESSION_DESKTOP",
        "DESKTOP_SESSION",
    ]
    .into_iter()
    .filter_map(|name| std::env::var(name).ok())
    .collect::<Vec<_>>()
    .join(":");
    let desktop = desktop.to_ascii_lowercase();
    desktop.contains("gnome") || desktop.contains("ubuntu")
}

#[cfg(target_os = "linux")]
fn gsettings_strings(schema: &str, key: &str) -> Option<Vec<String>> {
    let output = Command::new("gsettings")
        .args(["get", schema, key])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(parse_gvariant_strings(&String::from_utf8_lossy(
        &output.stdout,
    )))
}

#[cfg(target_os = "linux")]
fn gsettings_set(schema: &str, key: &str, value: &str) -> bool {
    Command::new("gsettings")
        .args(["set", schema, key, value])
        .output()
        .is_ok_and(|output| output.status.success())
}

#[cfg(target_os = "linux")]
fn parse_gvariant_strings(value: &str) -> Vec<String> {
    let mut strings = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut escaped = false;
    for character in value.chars() {
        if let Some(delimiter) = quote {
            if escaped {
                current.push(character);
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == delimiter {
                strings.push(std::mem::take(&mut current));
                quote = None;
            } else {
                current.push(character);
            }
        } else if matches!(character, '\'' | '"') {
            quote = Some(character);
        }
    }
    strings
}

#[cfg(target_os = "linux")]
fn gvariant_string(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

#[cfg(target_os = "linux")]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(target_os = "linux")]
fn mark_global_shortcuts(app: &AppHandle) {
    let state = app.state::<AppState>();
    lock(&state.portal_stop).cancel();
    lock(&state.capabilities).global_shortcuts = true;
    let _ = app.emit("capabilities-changed", ());
}

fn shortcut_error() -> AppError {
    AppError::new(
        ErrorCode::Configuration,
        "快捷键已被其他程序占用，请重新设置",
    )
    .action("hotkeys")
}

pub fn configure_portal(app: AppHandle, keys: Hotkeys) {
    #[cfg(target_os = "linux")]
    {
        let bindings = gnome_bindings(&keys);
        if bindings
            .as_ref()
            .is_ok_and(|bindings| sync_gnome_shortcuts(bindings))
        {
            mark_global_shortcuts(&app);
            return;
        }
        let state = app.state::<AppState>();
        lock(&state.portal_stop).cancel();
        let stop = tokio_util::sync::CancellationToken::new();
        *lock(&state.portal_stop) = stop.clone();
        tauri::async_runtime::spawn(async move {
            let callback_app = app.clone();
            let callback = std::sync::Arc::new(move |action: String| {
                if action == "ready" {
                    lock(&callback_app.state::<AppState>().capabilities).global_shortcuts = true;
                    let _ = callback_app.emit("capabilities-changed", ());
                } else {
                    dispatch(&callback_app, &action);
                }
            });
            if let Err(error) = screentrans_platform::portal::shortcuts(keys, callback, stop).await
            {
                app.state::<AppState>().warning(&app, error.message);
            }
        });
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (app, keys);
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::{gnome_binding, parse_gvariant_strings};

    #[test]
    fn converts_product_hotkeys_to_gnome_accelerators() {
        assert_eq!(
            gnome_binding("Ctrl+Alt+Q").as_deref(),
            Some("<Control><Alt>q")
        );
        assert_eq!(
            gnome_binding("Ctrl+Alt+Space").as_deref(),
            Some("<Control><Alt>space")
        );
        assert_eq!(
            gnome_binding("Super+Shift+F12").as_deref(),
            Some("<Super><Shift>F12")
        );
    }

    #[test]
    fn preserves_unrelated_gnome_custom_keybindings() {
        assert_eq!(
            parse_gvariant_strings(
                "['/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/', \
                 '/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom1/']"
            ),
            vec![
                "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom0/",
                "/org/gnome/settings-daemon/plugins/media-keys/custom-keybindings/custom1/",
            ]
        );
    }
}

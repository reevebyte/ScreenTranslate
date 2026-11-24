pub mod capture;
pub mod cloud_ocr;
pub mod credentials;
pub mod custom_ocr;
#[cfg(target_os = "macos")]
mod macos;
pub mod ocr;
#[cfg(target_os = "linux")]
pub mod portal;
pub mod rapid_ocr;

use screentrans_core::PlatformCapabilities;

/// The desktop session is Wayland. Screenshots and global hotkeys must then go
/// through the compositor (portal, GNOME settings), even when our own windows
/// run on XWayland.
pub fn is_wayland() -> bool {
    cfg!(target_os = "linux")
        && std::env::var_os("WAYLAND_DISPLAY").is_some()
        && std::env::var("XDG_SESSION_TYPE").as_deref() != Ok("x11")
}

/// Opt-out for users who prefer native Wayland windows over XWayland.
pub const NATIVE_WAYLAND_ENV: &str = "SCREENTRANSLATE_NATIVE_WAYLAND";

/// Opt-in for users on tiling Wayland compositors who explicitly want XWayland
/// overlay windows (paired with compositor window rules).
pub const XWAYLAND_ENV: &str = "SCREENTRANSLATE_XWAYLAND";

fn is_tiling_desktop_name(desktop: &str) -> bool {
    let lower = desktop.to_ascii_lowercase();
    ["hyprland", "sway", "niri", "river", "wayfire"]
        .iter()
        .any(|name| lower.contains(name))
}

/// Whether the current Wayland session is a tiling compositor where XWayland
/// clients cannot reliably position or pin floating overlay windows without
/// explicit user-configured compositor rules.
pub fn is_tiling_wayland_compositor() -> bool {
    if !is_wayland() {
        return false;
    }
    if std::env::var_os("HYPRLAND_INSTANCE_SIGNATURE").is_some()
        || std::env::var_os("SWAYSOCK").is_some()
        || std::env::var_os("NIRI_SOCKET").is_some()
    {
        return true;
    }
    std::env::var("XDG_CURRENT_DESKTOP")
        .ok()
        .is_some_and(|desktop| is_tiling_desktop_name(&desktop))
}

fn prefer_xwayland_from(
    wayland: bool,
    has_display: bool,
    has_gdk_backend: bool,
    native_wayland_opt_out: bool,
    force_xwayland: bool,
    tiling_compositor: bool,
) -> bool {
    if !wayland || !has_display || has_gdk_backend || native_wayland_opt_out {
        return false;
    }
    if force_xwayland {
        return true;
    }
    !tiling_compositor
}

/// Whether the app should switch GTK to XWayland before it starts. On stacking
/// Wayland desktops (GNOME, KDE), XWayland windows can be placed, kept on top
/// and cover a monitor. On tiling Wayland compositors (Hyprland, Sway, Niri),
/// default to native Wayland windows unless `SCREENTRANSLATE_XWAYLAND=1` is set.
pub fn prefer_xwayland() -> bool {
    prefer_xwayland_from(
        is_wayland(),
        std::env::var_os("DISPLAY").is_some_and(|value| !value.is_empty()),
        std::env::var_os("GDK_BACKEND").is_some(),
        std::env::var(NATIVE_WAYLAND_ENV).as_deref() == Ok("1"),
        std::env::var(XWAYLAND_ENV).as_deref() == Ok("1"),
        is_tiling_wayland_compositor(),
    )
}

/// Our windows are native Wayland surfaces: the app cannot position them and
/// has to fall back to the compositor's screenshot picker and centred results.
pub fn native_wayland_windows() -> bool {
    is_wayland()
        && !std::env::var("GDK_BACKEND").is_ok_and(|backend| {
            backend
                .split(',')
                .next()
                .is_some_and(|first| first.trim() == "x11")
        })
}

pub fn activate_app() {
    #[cfg(target_os = "macos")]
    macos::activate_app();
}
pub fn hide_dock_icon() {
    #[cfg(target_os = "macos")]
    macos::hide_dock_icon();
}
pub fn focus_app() {
    #[cfg(target_os = "macos")]
    macos::focus_app();
}
/// # Safety
/// `window_ptr` must be a valid `NSWindow *` pointer accessed on the AppKit main thread.
pub unsafe fn prepare_selection_window(window_ptr: *mut std::ffi::c_void) {
    #[cfg(target_os = "macos")]
    unsafe {
        macos::prepare_selection_window(window_ptr);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window_ptr;
}
pub fn set_crosshair_cursor(active: bool) {
    #[cfg(target_os = "macos")]
    macos::set_crosshair_cursor(active);
    #[cfg(not(target_os = "macos"))]
    let _ = active;
}
pub fn mouse_button_down() -> bool {
    #[cfg(target_os = "macos")]
    return macos::mouse_button_down();
    #[cfg(not(target_os = "macos"))]
    return false;
}
/// # Safety
/// `parent` and `child` must be valid `NSWindow *` pointers accessed on the AppKit main thread.
pub unsafe fn attach_child_window(parent: *mut std::ffi::c_void, child: *mut std::ffi::c_void) {
    #[cfg(target_os = "macos")]
    unsafe {
        macos::attach_child_window(parent, child);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (parent, child);
}
/// # Safety
/// `parent` and `child` must be valid `NSWindow *` pointers accessed on the AppKit main thread.
pub unsafe fn detach_child_window(parent: *mut std::ffi::c_void, child: *mut std::ffi::c_void) {
    #[cfg(target_os = "macos")]
    unsafe {
        macos::detach_child_window(parent, child);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (parent, child);
}
/// # Safety
/// `window_ptr` must be a valid `NSWindow *` pointer accessed on the AppKit main thread.
pub unsafe fn configure_transparent_window(window_ptr: *mut std::ffi::c_void) {
    #[cfg(target_os = "macos")]
    unsafe {
        macos::configure_transparent_window(window_ptr);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = window_ptr;
}
#[cfg(target_os = "linux")]
fn parse_portal_color_scheme(output: &str) -> Option<bool> {
    let trimmed = output.trim();
    // Portal Settings.Read returns `variant uint32 1` for prefer-dark, `2` for prefer-light, `0` for no preference.
    let mut tokens = trimmed.split_whitespace();
    while let Some(token) = tokens.next() {
        if token == "uint32"
            && let Some(value) = tokens.next()
        {
            return match value {
                "1" => Some(true),
                "2" => Some(false),
                _ => None,
            };
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn parse_gsettings_color_scheme(output: &str) -> Option<bool> {
    let lower = output.trim().trim_matches('\'').to_ascii_lowercase();
    if lower.contains("prefer-dark") {
        Some(true)
    } else if lower.contains("prefer-light") {
        Some(false)
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn parse_gtk_theme_dark(theme: &str) -> Option<bool> {
    let lower = theme.trim().trim_matches('\'').to_ascii_lowercase();
    if lower.is_empty() {
        return None;
    }
    if lower.ends_with("-dark") || lower.ends_with(":dark") {
        Some(true)
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn parse_dbus_name_has_owner(output: &str) -> Option<bool> {
    let mut tokens = output.split_whitespace();
    while let Some(token) = tokens.next() {
        if token == "boolean"
            && let Some(value) = tokens.next()
        {
            return match value {
                "true" => Some(true),
                "false" => Some(false),
                _ => None,
            };
        }
    }
    None
}

#[cfg(target_os = "linux")]
fn has_status_notifier_watcher() -> Option<bool> {
    std::env::var_os("DBUS_SESSION_BUS_ADDRESS")?;
    let output = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--print-reply=literal",
            "--reply-timeout=500",
            "--dest=org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus.NameHasOwner",
            "string:org.kde.StatusNotifierWatcher",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let has_watcher = parse_dbus_name_has_owner(&String::from_utf8_lossy(&output.stdout))?;
    if !has_watcher {
        return Some(false);
    }
    let host_output = std::process::Command::new("dbus-send")
        .args([
            "--session",
            "--print-reply=literal",
            "--reply-timeout=500",
            "--dest=org.kde.StatusNotifierWatcher",
            "/StatusNotifierWatcher",
            "org.freedesktop.DBus.Properties.Get",
            "string:org.kde.StatusNotifierWatcher",
            "string:IsStatusNotifierHostRegistered",
        ])
        .output()
        .ok();
    if let Some(output) = host_output
        && output.status.success()
        && let Some(host_registered) =
            parse_dbus_name_has_owner(&String::from_utf8_lossy(&output.stdout))
    {
        return Some(host_registered);
    }
    Some(true)
}

#[cfg(target_os = "linux")]
fn valid_hex_color(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_matches(['"', '\'']);
    if trimmed.len() == 7
        && trimmed.starts_with('#')
        && trimmed[1..].chars().all(|c| c.is_ascii_hexdigit())
    {
        Some(trimmed.to_ascii_lowercase())
    } else {
        None
    }
}

#[cfg(target_os = "linux")]
fn parse_omarchy_colors_toml(contents: &str) -> (Option<bool>, Option<String>) {
    let mut dark = None;
    let mut accent = None;
    for raw_line in contents.lines() {
        let line = raw_line.split('#').next().unwrap_or("");
        let Some((key, raw_value)) = raw_line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = raw_value.trim().trim_matches(['"', '\'']);
        if key == "mode" {
            let mode = line
                .split_once('=')
                .map(|(_, v)| v.trim().trim_matches(['"', '\'']))
                .unwrap_or(value)
                .to_ascii_lowercase();
            if mode == "dark" {
                dark = Some(true);
            } else if mode == "light" {
                dark = Some(false);
            }
        } else if key == "accent" {
            // Strip inline comments only after the hex literal.
            let candidate = raw_value.trim();
            let unquoted = if let Some(rest) = candidate.strip_prefix('"') {
                rest.split('"').next().unwrap_or("")
            } else if let Some(rest) = candidate.strip_prefix('\'') {
                rest.split('\'').next().unwrap_or("")
            } else {
                candidate.split_whitespace().next().unwrap_or("")
            };
            if let Some(hex) = valid_hex_color(unquoted) {
                accent = Some(hex);
            }
        }
    }
    (dark, accent)
}

#[cfg(target_os = "linux")]
fn read_omarchy_theme() -> (Option<bool>, Option<String>) {
    let path = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(std::path::PathBuf::from)
                .map(|home| home.join(".config"))
        })
        .map(|dir| dir.join("omarchy/current/theme/colors.toml"));
    let Some(path) = path else {
        return (None, None);
    };
    let Ok(contents) = std::fs::read_to_string(path) else {
        return (None, None);
    };
    parse_omarchy_colors_toml(&contents)
}

pub fn system_accent() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        read_omarchy_theme().1
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}

pub fn system_dark() -> bool {
    #[cfg(target_os = "macos")]
    return macos::system_dark();
    #[cfg(target_os = "linux")]
    {
        if std::env::var_os("DBUS_SESSION_BUS_ADDRESS").is_some()
            && let Ok(output) = std::process::Command::new("dbus-send")
                .args([
                    "--session",
                    "--print-reply=literal",
                    "--reply-timeout=500",
                    "--dest=org.freedesktop.portal.Desktop",
                    "/org/freedesktop/portal/desktop",
                    "org.freedesktop.portal.Settings.Read",
                    "string:org.freedesktop.appearance",
                    "string:color-scheme",
                ])
                .output()
            && output.status.success()
            && let Some(dark) = parse_portal_color_scheme(&String::from_utf8_lossy(&output.stdout))
        {
            return dark;
        }
        if let Ok(output) = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "color-scheme"])
            .output()
            && output.status.success()
            && let Some(dark) =
                parse_gsettings_color_scheme(&String::from_utf8_lossy(&output.stdout))
        {
            return dark;
        }
        if let Ok(theme) = std::env::var("GTK_THEME")
            && let Some(dark) = parse_gtk_theme_dark(&theme)
        {
            return dark;
        }
        if let Ok(output) = std::process::Command::new("gsettings")
            .args(["get", "org.gnome.desktop.interface", "gtk-theme"])
            .output()
            && output.status.success()
            && let Some(dark) = parse_gtk_theme_dark(&String::from_utf8_lossy(&output.stdout))
        {
            return dark;
        }
        if let Some(dark) = read_omarchy_theme().0 {
            return dark;
        }
        false
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    return false;
}

/// # Safety
/// `window_ptr` must be a valid `NSWindow *` pointer accessed on the AppKit main thread.
pub unsafe fn configure_window(
    window_ptr: *mut std::ffi::c_void,
    theme: screentrans_core::config::Theme,
) {
    #[cfg(target_os = "macos")]
    unsafe {
        macos::configure_window(window_ptr, theme);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (window_ptr, theme);
}

pub fn capabilities() -> PlatformCapabilities {
    let wayland = is_wayland();
    let native_windows = native_wayland_windows();
    let mut notes = Vec::new();
    if native_windows {
        notes.push(
            "Wayland 原生窗口使用系统截图选择器，译文在独立结果窗显示；快捷键由桌面 Portal 或桌面自定义快捷键管理"
                .into(),
        );
    } else if wayland {
        notes.push(
            "Wayland 首次截图时系统可能询问一次截图权限，请选择允许并记住；快捷键由桌面 Portal 管理".into(),
        );
    }
    let (local_ocr, ocr_languages) = ocr::available_languages();
    if !local_ocr {
        notes.push(if cfg!(target_os = "linux") {
            "未找到 Tesseract 或语言包；请安装 tesseract 及对应语言数据（如 eng、chi_sim）".into()
        } else {
            "未找到可用的系统 OCR 语言，请安装系统 OCR 语言包".into()
        });
    }
    if cfg!(target_os = "macos") {
        notes.push("首次截图需要 macOS 屏幕录制权限；拒绝后可在系统设置中重新授权".into());
    }
    #[cfg(target_os = "linux")]
    let tray = has_status_notifier_watcher().unwrap_or_else(|| {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_lowercase();
        !(desktop.contains("gnome") || desktop.contains("ubuntu"))
    });
    #[cfg(not(target_os = "linux"))]
    let tray = true;
    PlatformCapabilities {
        platform: std::env::consts::OS.into(),
        wayland,
        window_positioning: !native_windows,
        screenshot: true,
        overlay: !native_windows,
        global_shortcuts: !wayland,
        local_ocr,
        ocr_languages,
        tray,
        system_accent: system_accent(),
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiling_wayland_compositors_default_to_native_wayland_with_xwayland_opt_in() {
        assert!(prefer_xwayland_from(true, true, false, false, false, false));
        assert!(!prefer_xwayland_from(true, true, false, false, false, true));
        assert!(prefer_xwayland_from(true, true, false, false, true, true));
        assert!(!prefer_xwayland_from(true, true, false, true, true, false));
        assert!(!prefer_xwayland_from(true, true, true, false, true, false));
        assert!(!prefer_xwayland_from(
            true, false, false, false, true, false
        ));
        assert!(!prefer_xwayland_from(
            false, true, false, false, true, false
        ));
    }

    #[test]
    fn recognizes_tiling_wayland_desktop_names() {
        assert!(is_tiling_desktop_name("Hyprland"));
        assert!(is_tiling_desktop_name("wlroots:sway"));
        assert!(is_tiling_desktop_name("niri"));
        assert!(!is_tiling_desktop_name("GNOME"));
        assert!(!is_tiling_desktop_name("KDE"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn parses_linux_theme_and_tray_dbus_responses() {
        assert_eq!(
            parse_portal_color_scheme("   variant       uint32 1\n"),
            Some(true)
        );
        assert_eq!(
            parse_portal_color_scheme("   variant       uint32 2\n"),
            Some(false)
        );
        assert_eq!(
            parse_portal_color_scheme("   variant       uint32 0\n"),
            None
        );
        assert_eq!(parse_gsettings_color_scheme("'prefer-dark'\n"), Some(true));
        assert_eq!(parse_gsettings_color_scheme("'default'\n"), None);
        assert_eq!(parse_gtk_theme_dark("Adwaita-dark"), Some(true));
        assert_eq!(parse_gtk_theme_dark("Adwaita:dark"), Some(true));
        assert_eq!(parse_gtk_theme_dark("Adwaita"), None);
        assert_eq!(parse_dbus_name_has_owner("   boolean true\n"), Some(true));
        assert_eq!(parse_dbus_name_has_owner("   boolean false\n"), Some(false));
        assert_eq!(
            parse_dbus_name_has_owner("   variant       boolean true\n"),
            Some(true)
        );
        assert_eq!(
            parse_dbus_name_has_owner("   variant       boolean false\n"),
            Some(false)
        );
        assert_eq!(parse_dbus_name_has_owner("error"), None);
        assert_eq!(
            parse_omarchy_colors_toml("mode = \"dark\"\naccent = \"#89B4FA\" # catppuccin\n"),
            (Some(true), Some("#89b4fa".into()))
        );
        assert_eq!(
            parse_omarchy_colors_toml("mode = 'light'\naccent = '#d45b37'\n"),
            (Some(false), Some("#d45b37".into()))
        );
        assert_eq!(
            parse_omarchy_colors_toml("mode = \"unknown\"\naccent = \"invalid\"\n"),
            (None, None)
        );
    }
}

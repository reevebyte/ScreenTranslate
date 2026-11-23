//! Native Linux `org.kde.StatusNotifierItem` + `com.canonical.dbusmenu` tray.
//!
//! Tauri's built-in Linux tray uses `libappindicator`, which hardcodes
//! `ItemIsMenu = true` and suppresses left-click activation events. Serving
//! StatusNotifierItem directly over D-Bus lets Waybar / KDE / SNI bars trigger
//! screenshot capture on left-click while preserving the full right-click menu.

use crate::{shortcuts, state::lock, update_ui, windows};
use futures_util::StreamExt;
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::AppHandle;
use zbus::{
    Connection, Proxy, interface,
    object_server::SignalEmitter,
    zvariant::{OwnedObjectPath, OwnedValue, Structure, Value},
};

const ITEM_PATH: &str = "/StatusNotifierItem";
const MENU_PATH: &str = "/MenuBar";
const WATCHER_SERVICE: &str = "org.kde.StatusNotifierWatcher";
const WATCHER_PATH: &str = "/StatusNotifierWatcher";

const MENU_MAIN: i32 = 1;
const MENU_CAPTURE: i32 = 2;
const MENU_QUICK: i32 = 3;
const MENU_SEPARATOR: i32 = 4;
const MENU_UPDATE: i32 = 5;
const MENU_QUIT: i32 = 6;

type Pixmap = (i32, i32, Vec<u8>);
type ToolTip = (String, Vec<Pixmap>, String, String);
type MenuLayoutNode = (i32, HashMap<String, OwnedValue>, Vec<OwnedValue>);

#[derive(Clone)]
struct TrayMenuState {
    english: bool,
    update_text: String,
    update_enabled: bool,
    revision: u32,
}

impl TrayMenuState {
    fn initial() -> Self {
        Self {
            english: false,
            update_text: format!("版本 {}", env!("CARGO_PKG_VERSION")),
            update_enabled: false,
            revision: 1,
        }
    }

    fn item_properties(&self, id: i32) -> Option<HashMap<String, OwnedValue>> {
        let mut props = HashMap::new();
        match id {
            0 => {
                props.insert(
                    "children-display".into(),
                    OwnedValue::from(zbus::zvariant::Str::from("submenu")),
                );
            }
            MENU_MAIN => {
                let label = if self.english {
                    "Open main window"
                } else {
                    "打开主窗口"
                };
                props.insert(
                    "label".into(),
                    OwnedValue::from(zbus::zvariant::Str::from(label)),
                );
                props.insert("enabled".into(), OwnedValue::from(true));
                props.insert("visible".into(), OwnedValue::from(true));
            }
            MENU_CAPTURE => {
                let label = if self.english {
                    "Capture and translate"
                } else {
                    "截图翻译"
                };
                props.insert(
                    "label".into(),
                    OwnedValue::from(zbus::zvariant::Str::from(label)),
                );
                props.insert("enabled".into(), OwnedValue::from(true));
                props.insert("visible".into(), OwnedValue::from(true));
            }
            MENU_QUICK => {
                let label = if self.english {
                    "Quick translate"
                } else {
                    "快速文字翻译"
                };
                props.insert(
                    "label".into(),
                    OwnedValue::from(zbus::zvariant::Str::from(label)),
                );
                props.insert("enabled".into(), OwnedValue::from(true));
                props.insert("visible".into(), OwnedValue::from(true));
            }
            MENU_SEPARATOR => {
                props.insert(
                    "type".into(),
                    OwnedValue::from(zbus::zvariant::Str::from("separator")),
                );
                props.insert("visible".into(), OwnedValue::from(true));
            }
            MENU_UPDATE => {
                props.insert(
                    "label".into(),
                    OwnedValue::from(zbus::zvariant::Str::from(self.update_text.clone())),
                );
                props.insert("enabled".into(), OwnedValue::from(self.update_enabled));
                props.insert("visible".into(), OwnedValue::from(true));
            }
            MENU_QUIT => {
                let label = if self.english { "Quit" } else { "退出" };
                props.insert(
                    "label".into(),
                    OwnedValue::from(zbus::zvariant::Str::from(label)),
                );
                props.insert("enabled".into(), OwnedValue::from(true));
                props.insert("visible".into(), OwnedValue::from(true));
            }
            _ => return None,
        }
        Some(props)
    }

    fn layout_for(&self, parent_id: i32, recursion_depth: i32) -> (u32, MenuLayoutNode) {
        let props = self.item_properties(parent_id).unwrap_or_default();
        let children = if parent_id == 0 && recursion_depth != 0 {
            [
                MENU_MAIN,
                MENU_CAPTURE,
                MENU_QUICK,
                MENU_SEPARATOR,
                MENU_UPDATE,
                MENU_QUIT,
            ]
            .into_iter()
            .filter_map(|id| {
                let child_props = self.item_properties(id)?;
                let node = Structure::from((id, child_props, Vec::<OwnedValue>::new()));
                OwnedValue::try_from(Value::from(node)).ok()
            })
            .collect()
        } else {
            Vec::new()
        };
        (self.revision, (parent_id, props, children))
    }
}

fn decode_argb_pixmap(png_bytes: &[u8]) -> Option<Pixmap> {
    let rgba = image::load_from_memory(png_bytes).ok()?.to_rgba8();
    let (width, height) = rgba.dimensions();
    let mut argb = Vec::with_capacity((width as usize) * (height as usize) * 4);
    for pixel in rgba.pixels() {
        argb.push(pixel[3]);
        argb.push(pixel[0]);
        argb.push(pixel[1]);
        argb.push(pixel[2]);
    }
    Some((width as i32, height as i32, argb))
}

fn app_icon_pixmaps() -> Vec<Pixmap> {
    [
        include_bytes!("../icons/32x32.png").as_slice(),
        include_bytes!("../icons/128x128.png").as_slice(),
    ]
    .into_iter()
    .filter_map(decode_argb_pixmap)
    .collect()
}

struct StatusNotifierItem {
    app: AppHandle,
    pixmaps: Vec<Pixmap>,
}

#[interface(name = "org.kde.StatusNotifierItem")]
impl StatusNotifierItem {
    #[zbus(property)]
    fn category(&self) -> &str {
        "ApplicationStatus"
    }

    #[zbus(property)]
    fn id(&self) -> &str {
        "io.github.reevebyte.screentranslate.v2"
    }

    #[zbus(property)]
    fn title(&self) -> &str {
        "ScreenTranslate"
    }

    #[zbus(property)]
    fn status(&self) -> &str {
        "Active"
    }

    #[zbus(property)]
    fn window_id(&self) -> u32 {
        0
    }

    #[zbus(property)]
    fn icon_name(&self) -> &str {
        ""
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> &str {
        ""
    }

    #[zbus(property)]
    fn icon_pixmap(&self) -> Vec<Pixmap> {
        self.pixmaps.clone()
    }

    #[zbus(property)]
    fn overlay_icon_name(&self) -> &str {
        ""
    }

    #[zbus(property)]
    fn overlay_icon_pixmap(&self) -> Vec<Pixmap> {
        Vec::new()
    }

    #[zbus(property)]
    fn attention_icon_name(&self) -> &str {
        ""
    }

    #[zbus(property)]
    fn attention_icon_pixmap(&self) -> Vec<Pixmap> {
        Vec::new()
    }

    #[zbus(property)]
    fn attention_movie_name(&self) -> &str {
        ""
    }

    #[zbus(property)]
    fn tool_tip(&self) -> ToolTip {
        (
            String::new(),
            Vec::new(),
            "ScreenTranslate · 截图即翻译".into(),
            String::new(),
        )
    }

    #[zbus(property)]
    fn item_is_menu(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn menu(&self) -> OwnedObjectPath {
        OwnedObjectPath::try_from(MENU_PATH).expect("valid static dbus object path")
    }

    fn activate(&self, _x: i32, _y: i32) {
        windows::begin_capture_from_tray(self.app.clone());
    }

    fn secondary_activate(&self, _x: i32, _y: i32) {
        let _ = windows::show_main(&self.app);
    }

    fn context_menu(&self, _x: i32, _y: i32) {}

    fn scroll(&self, _delta: i32, _orientation: &str) {}
}

struct DBusMenu {
    app: AppHandle,
    state: Arc<Mutex<TrayMenuState>>,
}

impl DBusMenu {
    fn trigger(&self, id: i32) {
        match id {
            MENU_MAIN => {
                let _ = windows::show_main(&self.app);
            }
            MENU_CAPTURE => windows::begin_capture_from_tray(self.app.clone()),
            MENU_QUICK => shortcuts::dispatch(&self.app, "quick"),
            MENU_UPDATE if lock(&self.state).update_enabled => {
                update_ui::request(&self.app, "tray");
            }
            MENU_QUIT => windows::request_quit(&self.app),
            _ => {}
        }
    }
}

#[interface(name = "com.canonical.dbusmenu")]
impl DBusMenu {
    #[zbus(property)]
    fn version(&self) -> u32 {
        3
    }

    #[zbus(property)]
    fn text_direction(&self) -> &str {
        "ltr"
    }

    #[zbus(property)]
    fn status(&self) -> &str {
        "normal"
    }

    #[zbus(property)]
    fn icon_theme_path(&self) -> Vec<String> {
        Vec::new()
    }

    fn get_layout(
        &self,
        parent_id: i32,
        recursion_depth: i32,
        _property_names: Vec<String>,
    ) -> (u32, MenuLayoutNode) {
        lock(&self.state).layout_for(parent_id, recursion_depth)
    }

    fn get_group_properties(
        &self,
        ids: Vec<i32>,
        _property_names: Vec<String>,
    ) -> Vec<(i32, HashMap<String, OwnedValue>)> {
        let state = lock(&self.state);
        ids.into_iter()
            .filter_map(|id| state.item_properties(id).map(|props| (id, props)))
            .collect()
    }

    fn get_property(&self, id: i32, name: String) -> zbus::fdo::Result<OwnedValue> {
        lock(&self.state)
            .item_properties(id)
            .and_then(|mut props| props.remove(&name))
            .ok_or_else(|| zbus::fdo::Error::InvalidArgs("unknown menu property".into()))
    }

    fn event(&self, id: i32, event_id: String, _data: Value<'_>, _timestamp: u32) {
        if event_id == "clicked" {
            self.trigger(id);
        }
    }

    fn event_group(&self, events: Vec<(i32, String, OwnedValue, u32)>) -> Vec<i32> {
        for (id, event_id, _, _) in events {
            if event_id == "clicked" {
                self.trigger(id);
            }
        }
        Vec::new()
    }

    fn about_to_show(&self, _id: i32) -> bool {
        false
    }

    fn about_to_show_group(&self, _ids: Vec<i32>) -> (Vec<i32>, Vec<i32>) {
        (Vec::new(), Vec::new())
    }

    #[zbus(signal)]
    async fn layout_updated(
        emitter: &SignalEmitter<'_>,
        revision: u32,
        parent: i32,
    ) -> zbus::Result<()>;
}

pub struct LinuxTrayHandle {
    connection: Connection,
    state: Arc<Mutex<TrayMenuState>>,
}

impl LinuxTrayHandle {
    pub fn update(&self, update_text: String, update_enabled: bool, english: bool) {
        let revision = {
            let mut state = lock(&self.state);
            if state.update_text == update_text
                && state.update_enabled == update_enabled
                && state.english == english
            {
                return;
            }
            state.update_text = update_text;
            state.update_enabled = update_enabled;
            state.english = english;
            state.revision = state.revision.saturating_add(1);
            state.revision
        };
        let connection = self.connection.clone();
        tauri::async_runtime::spawn(async move {
            if let Ok(iface) = connection
                .object_server()
                .interface::<_, DBusMenu>(MENU_PATH)
                .await
            {
                let _ = DBusMenu::layout_updated(iface.signal_emitter(), revision, 0).await;
            }
        });
    }
}

async fn register_with_watcher(connection: &Connection, service_name: &str) -> zbus::Result<()> {
    let proxy = Proxy::new(connection, WATCHER_SERVICE, WATCHER_PATH, WATCHER_SERVICE).await?;
    if let Ok(false) = proxy
        .get_property::<bool>("IsStatusNotifierHostRegistered")
        .await
    {
        return Err(zbus::Error::Failure(
            "no StatusNotifierHost registered".into(),
        ));
    }
    let _: () = proxy
        .call("RegisterStatusNotifierItem", &(service_name,))
        .await?;
    Ok(())
}

async fn spawn_watcher_monitor(connection: Connection, service_name: String) {
    let Ok(proxy) = Proxy::new(
        &connection,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .await
    else {
        return;
    };
    let Ok(mut stream) = proxy.receive_signal("NameOwnerChanged").await else {
        return;
    };
    while let Some(message) = stream.next().await {
        if let Ok((name, _old_owner, new_owner)) =
            message.body().deserialize::<(String, String, String)>()
            && name == WATCHER_SERVICE
            && !new_owner.is_empty()
        {
            let _ = register_with_watcher(&connection, &service_name).await;
        }
    }
}

pub fn setup(app: &AppHandle) -> Result<LinuxTrayHandle, String> {
    let app = app.clone();
    tauri::async_runtime::block_on(async move {
        tokio::time::timeout(Duration::from_millis(1500), async move {
            let service_name = format!("org.kde.StatusNotifierItem-{}-1", std::process::id());
            let state = Arc::new(Mutex::new(TrayMenuState::initial()));
            let item = StatusNotifierItem {
                app: app.clone(),
                pixmaps: app_icon_pixmaps(),
            };
            let menu = DBusMenu {
                app,
                state: state.clone(),
            };
            let connection = zbus::connection::Builder::session()
                .map_err(|e| e.to_string())?
                .name(service_name.as_str())
                .map_err(|e| e.to_string())?
                .serve_at(ITEM_PATH, item)
                .map_err(|e| e.to_string())?
                .serve_at(MENU_PATH, menu)
                .map_err(|e| e.to_string())?
                .build()
                .await
                .map_err(|e| e.to_string())?;
            register_with_watcher(&connection, &service_name)
                .await
                .map_err(|e| e.to_string())?;
            tauri::async_runtime::spawn(spawn_watcher_monitor(connection.clone(), service_name));
            Ok(LinuxTrayHandle { connection, state })
        })
        .await
        .map_err(|_| "注册 Linux StatusNotifierItem 托盘超时".to_string())?
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_embedded_app_icons_into_argb_pixmaps() {
        let pixmaps = app_icon_pixmaps();
        assert_eq!(pixmaps.len(), 2);
        assert_eq!((pixmaps[0].0, pixmaps[0].1), (32, 32));
        assert_eq!(pixmaps[0].2.len(), 32 * 32 * 4);
        assert_eq!((pixmaps[1].0, pixmaps[1].1), (128, 128));
        assert_eq!(pixmaps[1].2.len(), 128 * 128 * 4);
    }

    #[test]
    fn dbusmenu_layout_reflects_locale_and_update_state() {
        let mut state = TrayMenuState::initial();
        let (rev, (root_id, _, children)) = state.layout_for(0, -1);
        assert_eq!(rev, 1);
        assert_eq!(root_id, 0);
        assert_eq!(children.len(), 6);

        state.english = true;
        state.update_text = "Version 2.1.0".into();
        state.update_enabled = true;
        let props = state.item_properties(MENU_CAPTURE).unwrap();
        assert_eq!(
            props
                .get("label")
                .and_then(|v| String::try_from(v.clone()).ok())
                .as_deref(),
            Some("Capture and translate")
        );
        let update_props = state.item_properties(MENU_UPDATE).unwrap();
        assert_eq!(
            update_props
                .get("enabled")
                .and_then(|v| bool::try_from(v.clone()).ok()),
            Some(true)
        );
    }
}

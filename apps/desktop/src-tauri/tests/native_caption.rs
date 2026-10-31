#![cfg(windows)]
// Run native caption checks in a manifest-bearing integration executable.
mod update_ui {
    pub fn request(_: &tauri::AppHandle, _: &str) {}
    pub fn caption_colors(_: &tauri::AppHandle) -> (u32, u32) {
        (0xf5f5f5, 0x242424)
    }
    pub async fn on_ui<T: Send + 'static>(
        _: &tauri::AppHandle,
        _: impl FnOnce(tauri::AppHandle) -> Result<T, String> + Send + 'static,
    ) -> Result<T, String> {
        Err("Not invoked by native geometry tests".into())
    }
}
#[allow(dead_code)]
mod caption {
    include!("../src/update_caption.rs");
    #[cfg(test)]
    mod tests {
        use super::*;
        #[test]
        fn action_stays_left_of_system_buttons_at_each_scale() {
            for dpi in [96, 120, 144, 192] {
                let scale = dpi as f64 / 96.;
                let buttons = RECT {
                    left: (800. * scale) as i32,
                    top: 0,
                    right: (940. * scale) as i32,
                    bottom: (32. * scale) as i32,
                };
                let action = action_rect(buttons, dpi).unwrap();
                assert!(action.right < buttons.left);
                assert!(action.top >= buttons.top && action.bottom <= buttons.bottom);
                assert_eq!(action.right - action.left, (104. * scale).round() as i32);
                let close = dismiss_rect(action.right - action.left, dpi);
                let pill = pill_rect(action.right - action.left, dpi);
                assert!(close.left > pill.right && close.top < pill.top);
                assert_eq!(pill.right - pill.left, (88. * scale).round() as i32);
                assert!(close.left > (action.right - action.left) / 2 && close.top == 0);
                assert!(
                    close.right == action.right - action.left
                        && close.bottom <= action.bottom - action.top
                );
            }
            assert!(action_rect(RECT::default(), 96).is_none());
        }
    }

    #[cfg(test)]
    mod native_tests {
        use super::*;
        use std::{cell::Cell, rc::Rc};
        #[test]
        fn caption_control_keeps_client_geometry_capture_protection_and_only_emits_a_click() {
            unsafe {
                let hwnd = CreateWindowExW(
                    WS_EX_NOACTIVATE,
                    w!("STATIC"),
                    w!("ScreenTranslate isolated caption fixture"),
                    WS_OVERLAPPEDWINDOW,
                    -30000,
                    -30000,
                    956,
                    679,
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap();
                let count = Rc::new(Cell::new(0));
                let clicks = count.clone();
                let dismissed = Rc::new(Cell::new(0));
                let closed = dismissed.clone();
                attach(
                    hwnd,
                    Box::new(move || clicks.set(clicks.get() + 1)),
                    Box::new(move || closed.set(closed.get() + 1)),
                )
                .unwrap();
                let button = state(hwnd).unwrap().button.get();
                let mut before = RECT::default();
                GetClientRect(hwnd, &mut before).unwrap();
                let menu = GetSystemMenu(hwnd, false);
                let menu_items = GetMenuItemCount(menu);
                update(hwnd, true, true, CaptionMode::Download, None);
                SendMessageW(button, BM_CLICK, WPARAM(0), LPARAM(0));
                assert_eq!(count.get(), 1);
                update(hwnd, true, false, CaptionMode::Restart, Some(100));
                SendMessageW(button, BM_CLICK, WPARAM(0), LPARAM(0));
                assert_eq!(count.get(), 1);
                update(hwnd, true, true, CaptionMode::Downloading, Some(42));
                assert!(!state(hwnd).unwrap().enabled.get());
                SendMessageW(button, BM_CLICK, WPARAM(0), LPARAM(0));
                assert_eq!(count.get(), 1);
                SendMessageW(button, WM_TIMER, WPARAM(ANIMATION_TIMER), LPARAM(0));
                assert_eq!(state(hwnd).unwrap().frame.get(), 1);
                assert_eq!(GetMenuItemCount(menu), menu_items);
                for expected in [17, 0, 17] {
                    SetWindowDisplayAffinity(hwnd, WINDOW_DISPLAY_AFFINITY(expected)).unwrap();
                    inherit_capture(hwnd).unwrap();
                    let mut actual = 0;
                    GetWindowDisplayAffinity(button, &mut actual).unwrap();
                    assert_eq!(actual, expected);
                }
                // Show only the off-screen fixture so a progress sync does not hide it as an inactive owner.
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                assert!(bounds(hwnd).is_some());
                // Close remains usable while the main download action is disabled.
                let _ = SetWindowPos(
                    button,
                    None,
                    -30000,
                    -30000,
                    104,
                    24,
                    SWP_NOACTIVATE | SWP_NOZORDER,
                );
                let point = LPARAM((6 << 16) | 101);
                SendMessageW(button, WM_MOUSEMOVE, WPARAM(0), point);
                assert!(state(hwnd).unwrap().close_hover.get());
                SendMessageW(button, WM_LBUTTONDOWN, WPARAM(0), point);
                update(hwnd, true, false, CaptionMode::Downloading, Some(43));
                assert!(state(hwnd).unwrap().pressed.get());
                assert!(state(hwnd).unwrap().close_pressed.get());
                SendMessageW(button, WM_LBUTTONUP, WPARAM(0), point);
                assert_eq!(dismissed.get(), 1);
                assert_eq!(count.get(), 1);
                assert!(!state(hwnd).unwrap().visible.get());
                update(hwnd, false, false, CaptionMode::Download, None);
                assert!(!IsWindowVisible(button).as_bool());
                let mut after = RECT::default();
                GetClientRect(hwnd, &mut after).unwrap();
                assert_eq!((before.right, before.bottom), (after.right, after.bottom));
                DestroyWindow(hwnd).unwrap();
                assert!(!IsWindow(button).as_bool());
                assert!(state(hwnd).is_none());
            }
        }
        #[test]
        fn renders_smooth_pill_matching_light_and_dark_caption_backgrounds() {
            unsafe {
                let width = 156_i32;
                let height = 36_i32;
                let dpi = 144_u32;
                let action = CaptionAction {
                    owner: HWND::default(),
                    button: Cell::new(HWND::default()),
                    visible: Cell::new(true),
                    enabled: Cell::new(true),
                    english: Cell::new(false),
                    hover: Cell::new(false),
                    pressed: Cell::new(false),
                    close_pressed: Cell::new(false),
                    close_hover: Cell::new(false),
                    version: RefCell::new("2.0.7".into()),
                    caption: Cell::new(0xf5f5f5),
                    text: Cell::new(0x242424),
                    mode: Cell::new(CaptionMode::Restart),
                    progress: Cell::new(Some(100)),
                    frame: Cell::new(0),
                    animating: Cell::new(false),
                    clicked: Box::new(|| {}),
                    dismissed: Box::new(|| {}),
                };
                for (name, bg, fg) in [
                    ("light", 0xf5f5f5_u32, 0x242424_u32),
                    ("dark", 0x121212_u32, 0xe5e5e5_u32),
                ] {
                    action.caption.set(bg);
                    action.text.set(fg);
                    let mem_dc = CreateCompatibleDC(None);
                    let mut info = BITMAPINFO::default();
                    info.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
                    info.bmiHeader.biWidth = width;
                    info.bmiHeader.biHeight = -height;
                    info.bmiHeader.biPlanes = 1;
                    info.bmiHeader.biBitCount = 32;
                    let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
                    let dib = CreateDIBSection(mem_dc, &info, DIB_RGB_COLORS, &mut bits, None, 0)
                        .unwrap();
                    let old = SelectObject(mem_dc, HGDIOBJ(dib.0));
                    paint_dc(mem_dc, width, height, dpi, &action);
                    let slice = std::slice::from_raw_parts(
                        bits as *const u8,
                        (width * height * 4) as usize,
                    );
                    assert_eq!(slice[0], (bg & 0xff) as u8);
                    if let Ok(dir) = std::env::var("SCREENTRANS_CAPTION_PREVIEW_DIR") {
                        let mut rgba = Vec::with_capacity((width * height * 4) as usize);
                        for px in slice.chunks_exact(4) {
                            rgba.extend_from_slice(&[px[2], px[1], px[0], 255]);
                        }
                        let img =
                            image::RgbaImage::from_raw(width as u32, height as u32, rgba).unwrap();
                        let _ = std::fs::create_dir_all(&dir);
                        img.save(std::path::Path::new(&dir).join(format!("caption-{name}.png")))
                            .unwrap();
                    }
                    SelectObject(mem_dc, old);
                    let _ = DeleteObject(HGDIOBJ(dib.0));
                    let _ = DeleteDC(mem_dc);
                }
            }
        }
    }

    #[cfg(test)]
    mod language_caption_tests {
        use super::*;
        #[test]
        fn localized_labels_keep_the_same_compact_update_button_states() {
            assert_eq!(caption_label(CaptionMode::Download, None, true), "Update");
            assert_eq!(caption_label(CaptionMode::Restart, None, true), "Restart");
            assert_eq!(
                caption_label(CaptionMode::Downloading, Some(42), true),
                "Get 42%"
            );
            assert_eq!(caption_label(CaptionMode::Restart, None, false), "重启更新");
        }
    }
}

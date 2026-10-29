//! A resize-only native outline. Solid, tiny layered surfaces avoid moving a
//! WebView's previous raster while its viewport catches up with a new HWND size.
use crate::windows::window_error;
use screentrans_core::{Rect, Result};
use std::sync::{Arc, OnceLock, mpsc};
use tauri::{AppHandle, Manager, WebviewWindow};
use windows::{
    Win32::{
        Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, WPARAM},
        Graphics::Gdi::{CreateEllipticRgn, CreateSolidBrush, SetWindowRgn, UpdateWindow},
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::*,
    },
    core::w,
};

unsafe extern "system" fn outline_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

pub struct ResizeOutline {
    app: AppHandle,
    handles: Vec<isize>,
}
fn rectangles(b: Rect, s: f64) -> Vec<(i32, i32, i32, i32)> {
    let (x, y, r, d) = (
        b.x.round() as i32,
        b.y.round() as i32,
        b.right().round() as i32,
        b.bottom().round() as i32,
    );
    let line = (2.0 * s).round().max(2.0) as i32;
    let half = line / 2;
    let dot = (6.0 * s).round().max(6.0) as i32;
    let radius = dot / 2;
    let mut result = vec![
        (x - half, y - half, r - x + line, line),
        (x - half, d - half, r - x + line, line),
        (x - half, y - half, line, d - y + line),
        (r - half, y - half, line, d - y + line),
    ];
    for (cx, cy) in [
        (x, y),
        ((x + r) / 2, y),
        (r, y),
        (r, (y + d) / 2),
        (r, d),
        ((x + r) / 2, d),
        (x, d),
        (x, (y + d) / 2),
    ] {
        result.push((cx - radius, cy - radius, dot, dot));
    }
    result
}
// Commit the owner and every outline surface in one UI-thread transaction.
// Separate SetWindowPos calls can expose mixed old/new edges to the compositor.
fn position(handles: &[isize], b: Rect, s: f64, owner: Option<(isize, Rect)>) -> Result<()> {
    unsafe {
        let mut batch = BeginDeferWindowPos((handles.len() + usize::from(owner.is_some())) as i32)
            .map_err(|_| window_error())?;
        if let Some((raw, outer)) = owner {
            batch = DeferWindowPos(
                batch,
                HWND(raw as *mut _),
                None,
                outer.x.round() as i32,
                outer.y.round() as i32,
                outer.width.round() as i32,
                outer.height.round() as i32,
                SWP_NOACTIVATE | SWP_NOZORDER,
            )
            .map_err(|_| window_error())?;
        }
        for (&raw, (x, y, width, height)) in handles.iter().zip(rectangles(b, s)) {
            batch = DeferWindowPos(
                batch,
                HWND(raw as *mut _),
                HWND_TOPMOST,
                x,
                y,
                width,
                height,
                SWP_NOACTIVATE | SWP_NOOWNERZORDER | SWP_SHOWWINDOW,
            )
            .map_err(|_| window_error())?;
        }
        EndDeferWindowPos(batch).map_err(|_| window_error())?;
        for &raw in handles {
            let _ = UpdateWindow(HWND(raw as *mut _));
        }
    }
    Ok(())
}

fn create(owner: isize, b: Rect, s: f64) -> Result<Vec<isize>> {
    static REGISTERED: OnceLock<bool> = OnceLock::new();
    let instance = HINSTANCE(
        unsafe { GetModuleHandleW(None) }
            .map_err(|_| window_error())?
            .0,
    );
    if !REGISTERED.get_or_init(|| unsafe {
        let blue = WNDCLASSW {
            lpfnWndProc: Some(outline_proc),
            hInstance: instance,
            lpszClassName: w!("ScreenTranslateResizeBlue"),
            hbrBackground: CreateSolidBrush(COLORREF(0x00ff8b16)),
            ..Default::default()
        };
        let white = WNDCLASSW {
            lpfnWndProc: Some(outline_proc),
            hInstance: instance,
            lpszClassName: w!("ScreenTranslateResizeWhite"),
            hbrBackground: CreateSolidBrush(COLORREF(0x00ffffff)),
            ..Default::default()
        };
        RegisterClassW(&blue) != 0 && RegisterClassW(&white) != 0
    }) {
        return Err(window_error());
    }
    let mut handles = Vec::new();
    for i in 0..12 {
        let result = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                if i < 4 {
                    w!("ScreenTranslateResizeBlue")
                } else {
                    w!("ScreenTranslateResizeWhite")
                },
                w!(""),
                WS_POPUP | WS_DISABLED,
                0,
                0,
                1,
                1,
                HWND(owner as *mut _),
                None,
                instance,
                None,
            )
        };
        let hwnd = match result {
            Ok(hwnd) => hwnd,
            Err(_) => {
                destroy(&handles);
                return Err(window_error());
            }
        };
        handles.push(hwnd.0 as isize);
        unsafe {
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
            if i >= 4 {
                let size = (6.0 * s).round().max(6.0) as i32;
                SetWindowRgn(hwnd, CreateEllipticRgn(0, 0, size, size), true);
            }
        }
    }
    if let Err(error) = position(&handles, b, s, None) {
        destroy(&handles);
        return Err(error);
    }
    Ok(handles)
}
fn destroy(handles: &[isize]) {
    for &raw in handles {
        unsafe {
            let _ = DestroyWindow(HWND(raw as *mut _));
        }
    }
}
impl ResizeOutline {
    pub fn new(window: &WebviewWindow, b: Rect, s: f64) -> Result<Arc<Self>> {
        let app = window.app_handle().clone();
        let owner = window.hwnd().map_err(|_| window_error())?.0 as isize;
        let (tx, rx) = mpsc::sync_channel(1);
        app.run_on_main_thread(move || {
            if let Err(error) = tx.send(create(owner, b, s))
                && let Ok(handles) = error.0
            {
                destroy(&handles);
            }
        })
        .map_err(|_| window_error())?;
        let handles = rx
            .recv_timeout(std::time::Duration::from_secs(3))
            .map_err(|_| window_error())??;
        Ok(Arc::new(Self { app, handles }))
    }
    pub fn update(&self, window: &WebviewWindow, outer: Rect, b: Rect, s: f64) -> Result<()> {
        let handles = self.handles.clone();
        let owner = window.hwnd().map_err(|_| window_error())?.0 as isize;
        let (tx, rx) = mpsc::sync_channel(1);
        self.app
            .run_on_main_thread(move || {
                let _ = tx.send(position(&handles, b, s, Some((owner, outer))));
            })
            .map_err(|_| window_error())?;
        rx.recv_timeout(std::time::Duration::from_secs(3))
            .map_err(|_| window_error())?
    }
}
impl Drop for ResizeOutline {
    fn drop(&mut self) {
        let handles = self.handles.clone();
        let _ = self.app.run_on_main_thread(move || destroy(&handles));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_edges_and_handles_share_the_same_physical_pixels() {
        let a = rectangles(
            Rect {
                x: -100.0,
                y: 20.0,
                width: 320.0,
                height: 120.0,
            },
            1.5,
        );
        let b = rectangles(
            Rect {
                x: -70.0,
                y: 20.0,
                width: 290.0,
                height: 120.0,
            },
            1.5,
        );
        assert_eq!(a[3], b[3]);
        assert_eq!(a[6], b[6]);
        assert_eq!(a[7], b[7]);
        assert_eq!(a.len(), 12);
    }
}

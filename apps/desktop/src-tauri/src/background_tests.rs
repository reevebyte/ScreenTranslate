//! Real Wry regression: let the production reaper remove every native webview.
use super::*;
use std::{
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

fn hidden_window(app: &tauri::AppHandle, label: &str) -> tauri::Result<()> {
    tauri::WebviewWindowBuilder::new(
        app,
        label,
        tauri::WebviewUrl::External("about:blank".parse().unwrap()),
    )
    .visible(false)
    .build()?;
    Ok(())
}

#[test]
#[ignore = "requires an interactive Windows desktop; waits for two real reaper cycles"]
fn reaped_windows_keep_background_runtime_alive() {
    const CHILD: &str = "SCREENTRANS_BACKGROUND_TEST_CHILD";
    const COMPLETE: &str = "Explicit quit accepted after two reaper cycles";
    // Wry exits the process when its event loop ends. Keep the test harness in
    // a parent process so an unintended clean exit cannot masquerade as a pass.
    if std::env::var_os(CHILD).is_none() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "background_tests::reaped_windows_keep_background_runtime_alive",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD, "1")
            .output()
            .expect("start isolated native lifecycle process");
        let diagnostics = format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        eprintln!("{diagnostics}");
        assert!(output.status.success(), "native lifecycle process failed");
        assert!(diagnostics.contains(COMPLETE), "runtime exited prematurely");
        return;
    }
    let (implicit_tx, implicit_rx) = mpsc::channel();
    let completed = Arc::new(AtomicBool::new(false));
    let observed_complete = completed.clone();
    let app = tauri::Builder::default()
        .any_thread()
        .on_window_event(|window, event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                windows::window_destroyed(window);
            }
        })
        .setup(|app| {
            let handle = app.handle();
            app.manage(AppState::new(handle));
            app.manage(update_ui::UpdateUi::default());
            lock(&app.state::<AppState>().sessions).text.input = "retained session".into();
            hidden_window(handle, "main")?;
            windows::reap_hidden_windows(handle.clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("build isolated native lifecycle test");
    let handle = app.handle().clone();
    std::thread::spawn(move || {
        let result = (|| -> Result<(), String> {
            for label in ["quick", "main"] {
                implicit_rx
                    .recv_timeout(Duration::from_secs(95))
                    .map_err(|_| "last hidden window was not reaped".to_string())?;
                let (tx, rx) = mpsc::channel();
                let app = handle.clone();
                handle
                    .run_on_main_thread(move || {
                        let result = (|| -> Result<(), String> {
                            if !app.webview_windows().is_empty() {
                                return Err("expected zero webviews after reaping".into());
                            }
                            if lock(&app.state::<AppState>().sessions).text.input
                                != "retained session"
                            {
                                return Err("background session was lost".into());
                            }
                            hidden_window(&app, label).map_err(|e| e.to_string())?;
                            let window = app.get_webview_window(label).unwrap();
                            window.show().map_err(|e| e.to_string())?;
                            window.hide().map_err(|e| e.to_string())?;
                            Ok(())
                        })();
                        let _ = tx.send(result);
                    })
                    .map_err(|e| e.to_string())?;
                rx.recv_timeout(Duration::from_secs(15))
                    .map_err(|_| "runtime stopped before a window could reopen".to_string())??;
                eprintln!("Reaped all webviews; reopened {label}; session retained");
            }
            Ok(())
        })();
        completed.store(result.is_ok(), Ordering::SeqCst);
        if let Err(error) = &result {
            eprintln!("Background lifecycle failed: {error}");
        }
        handle.exit(if result.is_ok() { 0 } else { 1 });
    });
    app.run(move |app, event| {
        if let tauri::RunEvent::ExitRequested { code, .. } = &event {
            match code {
                None => {
                    let _ = implicit_tx.send(());
                }
                Some(0) if observed_complete.load(Ordering::SeqCst) => eprintln!("{COMPLETE}"),
                Some(_) => {}
            }
        }
        handle_run_event(app, event);
    });
}

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(model_dir) = screentranslate_desktop::parse_ocr_worker_args() {
        std::process::exit(screentrans_platform::rapid_ocr::worker_main(&model_dir));
    }
    screentranslate_desktop::run();
}

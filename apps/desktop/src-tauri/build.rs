fn main() {
    tauri_build::build();
    // Rust test executables also link native menus/dialogs; they need ComCtl32 v6.
    #[cfg(windows)]
    {
        // `rustc-link-arg-tests` excludes lib unit-test executables.
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg=/MANIFESTDEPENDENCY:type='win32' name='Microsoft.Windows.Common-Controls' version='6.0.0.0' processorArchitecture='*' publicKeyToken='6595b64144ccf1df' language='*'"
        );
        // Tauri already supplies the application's full manifest in resource.lib.
        println!("cargo:rustc-link-arg-bin=screentranslate=/MANIFEST:NO");
    }
}

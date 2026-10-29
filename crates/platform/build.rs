fn main() {
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        cc::Build::new()
            .file("native/macos.m")
            .flag("-fobjc-arc")
            .flag("-fblocks")
            .flag("-mmacosx-version-min=14.0")
            .compile("screentrans_macos");
        for framework in [
            "Foundation",
            "AppKit",
            "Vision",
            "ScreenCaptureKit",
            "CoreGraphics",
        ] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    }
    println!("cargo:rerun-if-changed=native/macos.m");
}

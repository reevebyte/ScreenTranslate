//! Compatibility shim for Tauri's updater, which still requests `zip` 4.x.
//!
//! The updater only uses `ZipArchive::new` and `ZipArchive::extract`. Keeping
//! that tiny surface here lets it use the patched `zip` 9 implementation
//! without vendoring the whole updater crate.

pub use zip_next::*;

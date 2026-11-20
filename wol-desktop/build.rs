use std::fs;
use std::path::Path;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let root = Path::new(&manifest_dir);
    let ui_dir = root.join("ui");
    let ui_lib = ui_dir.join("lib");
    let ui_icons = ui_dir.join("icons");

    let _ = fs::create_dir_all(&ui_lib);
    let _ = fs::create_dir_all(&ui_icons);

    let src_lib = root.join("../wol-highlights/lib");
    let src_icons = root.join("../wol-highlights/icons");
    let src_root = root.join("../wol-highlights");

    let files_to_copy = [
        (src_lib.join("sql-wasm.js"), ui_lib.join("sql-wasm.js")),
        (src_lib.join("sql-wasm.wasm"), ui_lib.join("sql-wasm.wasm")),
        (src_lib.join("backup.js"), ui_lib.join("backup.js")),
        (src_lib.join("tokenize.js"), ui_lib.join("tokenize.js")),
        (src_lib.join("highlight.js"), ui_lib.join("highlight.js")),
        (src_icons.join("icon-48.png"), ui_icons.join("icon-48.png")),
        (src_root.join("popup.css"), ui_dir.join("popup.css")),
    ];

    for (src, dest) in files_to_copy {
        if src.exists() {
            let _ = fs::copy(&src, &dest);
        }
    }

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("ios") {
        println!("cargo:rerun-if-changed=src/ios_auth.m");
        cc::Build::new()
            .file("src/ios_auth.m")
            .flag("-fobjc-arc")
            .compile("wol_ios_auth");
        println!("cargo:rustc-link-lib=framework=AuthenticationServices");
        println!("cargo:rustc-link-lib=framework=UIKit");
    }

    tauri_build::build()
}

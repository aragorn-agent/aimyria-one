fn main() {
    tauri_build::build();

    // Emitting any `cargo:rerun-if-changed` switches Cargo from "rerun when any
    // file in the package changes" to "rerun only when one of these changes".
    //
    // The manifest line below therefore turned off the default, and with it the
    // detection of icon and `tauri.conf.json` edits — so replacing the bundled
    // icons produced a binary with the OLD icon still embedded, silently. The
    // build succeeded, the size was right, and the taskbar glyph was the one we
    // had just deleted. Confirmed by `.rsrc` staying at 0x1b60 across a rebuild
    // that had genuinely new icon files.
    //
    // Named explicitly so the next asset change is picked up.
    let manifest_dir = std::path::PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"),
    );

    // The renderer is embedded into the binary by `tauri_build`, which reads
    // `dist/` at build time. Nothing else in the package changes when the frontend
    // does, so without this line a `pnpm run build:renderer` leaves the exe
    // carrying the previous bundle — a green build, a correct-looking binary, and
    // the old UI at runtime. The dist directory itself is listed rather than each
    // hashed asset, so a rebuild with new content-hashed filenames still counts as
    // a change.
    println!(
        "cargo:rerun-if-changed={}",
        manifest_dir.join("dist").display()
    );

    for rel in [
        "tauri.conf.json",
        "icons/icon.ico",
        "icons/icon.png",
        "icons/32x32.png",
        "icons/128x128.png",
        "icons/128x128@2x.png",
        "icons/Square30x30Logo.png",
        "icons/Square44x44Logo.png",
        "icons/Square71x71Logo.png",
        "icons/Square89x89Logo.png",
        "icons/Square107x107Logo.png",
        "icons/Square142x142Logo.png",
        "icons/Square150x150Logo.png",
        "icons/Square284x284Logo.png",
        "icons/Square310x310Logo.png",
        "icons/StoreLogo.png",
    ] {
        let p = manifest_dir.join(rel);
        if p.exists() {
            println!("cargo:rerun-if-changed={}", p.display());
        }
    }

    // MSVC-only: embed Common Controls v6 manifest into test binaries.
    //
    // When running `cargo test`, the generated test executables don't include
    // the standard Tauri application manifest. Without Common Controls v6,
    // `tauri::test` calls fail with STATUS_ENTRYPOINT_NOT_FOUND.
    //
    // This workaround:
    // 1. Embeds the manifest into test binaries via /MANIFEST:EMBED
    // 2. Uses /MANIFEST:NO for the main binary to avoid duplicate resources
    //    (Tauri already handles manifest embedding for the app binary)
    //
    // NOTE (Aimyria One): the guard is `all(windows, msvc)`, not just `windows`.
    // `/MANIFEST:EMBED` and `/MANIFESTINPUT:` are MSVC linker flags; GNU ld
    // rejects them with "cannot find /MANIFEST:EMBED". The main binary still
    // gets its manifest from tauri's own `set_manifest` (compiled via
    // windres into a .rc), which works on both toolchains.
    #[cfg(all(target_os = "windows", target_env = "msvc"))]
    {
        let manifest_path = std::path::PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR").expect("missing CARGO_MANIFEST_DIR"),
        )
        .join("common-controls.manifest");
        let manifest_arg = format!("/MANIFESTINPUT:{}", manifest_path.display());

        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg={}", manifest_arg);
        // Avoid duplicate manifest resources in binary builds.
        println!("cargo:rustc-link-arg-bins=/MANIFEST:NO");
        println!("cargo:rerun-if-changed={}", manifest_path.display());
    }
}

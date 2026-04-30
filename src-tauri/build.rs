// Tauri's `bundle.externalBin` pipeline doesn't reliably place sidecars where
// `tauri_plugin_shell::ShellExt::sidecar()` looks for them in dev mode. The
// runtime resolver does `current_exe.parent() / <name>` (no triple-suffix
// appending), so for sidecar `"binaries/yt-dlp"` it expects
// `target/<profile>/binaries/yt-dlp`. We mirror that here.
//
// Source files at `src-tauri/binaries/<name>-<host-triple>` get symlinked to
// `target/<profile>/binaries/<name>` for every sidecar declared in
// tauri.conf.json. Idempotent; rerun on source binary changes.

use std::path::PathBuf;

const SIDECARS: &[&str] = &["yt-dlp", "ffmpeg", "whisper-cli"];

fn main() {
    tauri_build::build();
    if let Err(e) = link_sidecars() {
        // Don't fail the build — just print a warning. Manually-placed
        // binaries in target/<profile>/binaries/ still work.
        println!("cargo:warning=sidecar link step failed: {e}");
    }
}

fn link_sidecars() -> std::io::Result<()> {
    let target_triple =
        std::env::var("TARGET").unwrap_or_else(|_| "x86_64-apple-darwin".into());
    let profile = std::env::var("PROFILE").unwrap_or_else(|_| "debug".into());

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let source_dir = manifest_dir.join("binaries");
    let profile_dir = manifest_dir.join("target").join(&profile);
    let target_dir = profile_dir.join("binaries");

    std::fs::create_dir_all(&target_dir)?;

    let exe_suffix = if target_triple.contains("windows") { ".exe" } else { "" };

    for name in SIDECARS {
        let src_name = format!("{name}-{target_triple}{exe_suffix}");
        let dst_name = format!("{name}{exe_suffix}");
        let src = source_dir.join(&src_name);
        let dst = target_dir.join(&dst_name);

        println!("cargo:rerun-if-changed={}", src.display());

        if !src.exists() {
            println!(
                "cargo:warning=sidecar source not found: {} (skipped)",
                src.display()
            );
            continue;
        }

        let _ = std::fs::remove_file(&dst);

        #[cfg(unix)]
        std::os::unix::fs::symlink(&src, &dst)?;

        #[cfg(windows)]
        std::fs::copy(&src, &dst).map(|_| ())?;
    }

    // whisper-cli is built with rpath `@loader_path/../lib`. dyld resolves
    // @loader_path to the directory of the **real** binary on disk (it
    // follows symlinks), so when the symlink chain lands at
    // `src-tauri/binaries/whisper-cli-<triple>`, dyld looks for the dylibs at
    // `src-tauri/lib/`. We also stage `target/<profile>/lib/` for safety in
    // case any tooling invokes the binary from there directly.
    #[cfg(target_os = "macos")]
    {
        stage_whisper_dylibs_macos(&profile_dir.join("lib"))?;
        stage_whisper_dylibs_macos(&manifest_dir.join("lib"))?;
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn stage_whisper_dylibs_macos(lib_dir: &std::path::Path) -> std::io::Result<()> {
    use std::process::Command;

    // Find brew prefix robustly so this works on Apple Silicon (/opt/homebrew)
    // and Intel (/usr/local) without hardcoding.
    let brew_prefix = Command::new("brew")
        .args(["--prefix", "whisper-cpp"])
        .output()
        .ok()
        .and_then(|o| {
            if o.status.success() {
                String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
            } else {
                None
            }
        });

    // If the symlink already points to a valid lib/ dir, nothing to do.
    if let Ok(meta) = std::fs::symlink_metadata(lib_dir) {
        if meta.file_type().is_symlink() {
            if std::fs::canonicalize(lib_dir).is_ok() {
                return Ok(());
            }
            std::fs::remove_file(lib_dir)?;
        } else if meta.is_dir() {
            // A real directory exists (e.g. produced by a future bundle
            // packaging step); leave it alone.
            return Ok(());
        }
    }

    let Some(prefix) = brew_prefix else {
        println!(
            "cargo:warning=brew not found or whisper-cpp not installed; whisper-cli may fail at runtime"
        );
        return Ok(());
    };
    let src_lib = std::path::Path::new(&prefix).join("libexec").join("lib");
    if !src_lib.exists() {
        println!(
            "cargo:warning=expected brew lib dir missing: {} (whisper-cli may fail)",
            src_lib.display()
        );
        return Ok(());
    }

    if let Some(parent) = lib_dir.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::os::unix::fs::symlink(&src_lib, lib_dir)?;
    println!(
        "cargo:warning=staged whisper dylibs: {} -> {}",
        lib_dir.display(),
        src_lib.display()
    );
    Ok(())
}

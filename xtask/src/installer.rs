//! `cargo xtask icon` writes the app icon file; `cargo xtask installer-windows` builds the
//! Windows installer on Linux (docs/install-windows.md): the release binaries cross-compiled for
//! `x86_64-pc-windows-gnu` with MinGW-w64, staged with the icon and the licence, and packed by
//! NSIS's `makensis` from `apps/emcraft/installer/emcraft.nsi`.
//!
//! The tools come from Nix when they are not on `PATH`: the command runs itself again inside
//! `nix-shell -p` [`NIX_PACKAGES`].

use std::path::{Path, PathBuf};
use std::process::Command;

/// The icon file, relative to the workspace root (checked in; `cargo xtask icon` writes it).
pub const ICON: &str = "apps/emcraft/assets/emcraft.ico";

/// The installer script, relative to the workspace root.
const SCRIPT: &str = "apps/emcraft/installer/emcraft.nsi";

const TARGET: &str = "x86_64-pc-windows-gnu";
const GCC: &str = "x86_64-w64-mingw32-gcc";
const WINDRES: &str = "x86_64-w64-mingw32-windres";

/// The Nix packages giving MinGW-w64's GCC (with binutils' `windres`), its threads library and
/// NSIS.
pub const NIX_PACKAGES: &[&str] = &["pkgsCross.mingwW64.buildPackages.gcc", "pkgsCross.mingwW64.windows.pthreads", "nsis"];

/// Set when the command runs inside the `nix-shell` it started, so it does not start another.
const IN_NIX: &str = "EMCRAFT_INSTALLER_NIX";

/// Writes [`ICON`] from `emcraft_render::icon`.
pub fn icon(root: &Path) -> Result<(), String> {
    let bytes = emcraft_render::icon::ico().ok_or("icon: the PNG encoder failed")?;
    let path = root.join(ICON);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    println!("icon: wrote {} ({} bytes)", path.display(), bytes.len());
    Ok(())
}

/// The installer's licence page: which licences apply, then both texts, with Windows line ends.
pub fn licence_text(root: &Path) -> Result<String, String> {
    let read = |name: &str| std::fs::read_to_string(root.join(name)).map_err(|e| format!("{name}: {e}"));
    let text = format!(
        "EMCraft is licensed under either of the MIT licence or the Apache License, Version 2.0, at your option.\n\n\
         ==============================================================================\n\n{}\n\n\
         ==============================================================================\n\n{}",
        read("LICENSE-MIT")?.trim_end(),
        read("LICENSE-APACHE")?.trim_end()
    );
    Ok(text.replace("\r\n", "\n").replace('\n', "\r\n") + "\r\n")
}

/// `VERSION` as Windows' four-number file version: `0.1.0` → `0.1.0.0`; a pre-release or build
/// suffix is dropped. `None` when it does not start with three numbers.
pub fn file_version(version: &str) -> Option<String> {
    let core = version.split(['-', '+']).next()?;
    let parts: Vec<u16> = core.split('.').map(|p| p.parse().ok()).collect::<Option<_>>()?;
    (parts.len() == 3).then(|| format!("{}.{}.{}.0", parts[0], parts[1], parts[2]))
}

/// The first `name` on `PATH`.
fn on_path(name: &str) -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?).map(|d| d.join(name)).find(|p| p.is_file())
}

fn nix_shell() -> Option<PathBuf> {
    on_path("nix-shell").or_else(|| Some(PathBuf::from("/nix/var/nix/profiles/default/bin/nix-shell")).filter(|p| p.is_file()))
}

/// `s` quoted for a POSIX shell.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// `cargo xtask installer-windows [--out FILE]`.
pub fn windows(root: &Path, rest: &[&str]) -> Result<(), String> {
    let mut out: Option<PathBuf> = None;
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match *a {
            "--out" => out = Some(PathBuf::from(it.next().ok_or("installer-windows: --out needs a file")?)),
            other => return Err(format!("installer-windows: unknown argument `{other}`")),
        }
    }
    if on_path(GCC).is_none() || on_path("makensis").is_none() {
        if std::env::var_os(IN_NIX).is_some() {
            return Err(format!("installer-windows: {GCC} or makensis missing inside nix-shell"));
        }
        let nix = nix_shell().ok_or(format!("installer-windows: needs {GCC} (MinGW-w64) and makensis (NSIS) on PATH, or Nix to fetch them"))?;
        let me = std::env::current_exe().map_err(|e| format!("installer-windows: {e}"))?;
        let mut line = vec![sh_quote(&me.to_string_lossy()), "installer-windows".into()];
        line.extend(rest.iter().map(|a| sh_quote(a)));
        let mut c = Command::new(nix);
        c.arg("-p").args(NIX_PACKAGES).arg("--run").arg(line.join(" ")).env(IN_NIX, "1").current_dir(root);
        return crate::run(c, "nix-shell (MinGW-w64, NSIS)");
    }
    let (version, target_dir) = metadata()?;
    let file_version = file_version(&version).ok_or(format!("installer-windows: version `{version}` is not MAJOR.MINOR.PATCH"))?;
    build(root, &target_dir)?;

    let stage = target_dir.join("installer-windows");
    std::fs::create_dir_all(&stage).map_err(|e| format!("{}: {e}", stage.display()))?;
    let bins = target_dir.join(TARGET).join("release");
    for exe in ["emcraft.exe", "emcraft-cli.exe"] {
        std::fs::copy(bins.join(exe), stage.join(exe)).map_err(|e| format!("{exe}: {e}"))?;
    }
    std::fs::copy(root.join(ICON), stage.join("emcraft.ico")).map_err(|e| format!("{ICON}: {e}"))?;
    std::fs::write(stage.join("LICENSE.txt"), licence_text(root)?).map_err(|e| format!("LICENSE.txt: {e}"))?;

    let out = match out {
        Some(p) if p.is_absolute() => p,
        Some(p) => std::env::current_dir().map_err(|e| e.to_string())?.join(p),
        None => target_dir.join(format!("EMCraft-{version}-setup.exe")),
    };
    let mut nsis = Command::new("makensis");
    nsis.arg("-V2")
        .arg("-INPUTCHARSET")
        .arg("UTF8")
        .arg(format!("-DVERSION={version}"))
        .arg(format!("-DFILE_VERSION={file_version}"))
        .arg(format!("-DSTAGE={}", stage.display()))
        .arg(format!("-DOUTFILE={}", out.display()))
        .arg(root.join(SCRIPT))
        .current_dir(root);
    crate::run(nsis, "makensis")?;
    let size = std::fs::metadata(&out).map_err(|e| format!("{}: {e}", out.display()))?.len();
    println!("installer-windows: {} ({:.1} MB)", out.display(), size as f64 / 1e6);
    Ok(())
}

/// The app's version and cargo's target directory.
fn metadata() -> Result<(String, PathBuf), String> {
    let out = crate::cargo().args(["metadata", "--format-version", "1", "--no-deps"]).output().map_err(|e| format!("cargo metadata: {e}"))?;
    if !out.status.success() {
        return Err(format!("cargo metadata failed: {}", String::from_utf8_lossy(&out.stderr)));
    }
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).map_err(|e| format!("cargo metadata: {e}"))?;
    let version = meta["packages"]
        .as_array()
        .and_then(|ps| ps.iter().find(|p| p["name"] == "emcraft"))
        .and_then(|p| p["version"].as_str())
        .ok_or("cargo metadata: no emcraft package")?
        .to_owned();
    let target = meta["target_directory"].as_str().ok_or("cargo metadata: no target directory")?;
    Ok((version, PathBuf::from(target)))
}

/// The release binaries for Windows, linked by MinGW-w64's GCC.
fn build(root: &Path, target_dir: &Path) -> Result<(), String> {
    if on_path("rustup").is_some() {
        let mut add = Command::new("rustup");
        add.args(["target", "add", TARGET]);
        crate::run(add, "rustup target add")?;
    }
    let mut c = crate::cargo();
    c.args(["build", "--release", "--target", TARGET, "-p", "emcraft", "-p", "emcraft-cli"])
        .env("CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER", GCC)
        .env("CC_x86_64_pc_windows_gnu", GCC)
        .env("AR_x86_64_pc_windows_gnu", "x86_64-w64-mingw32-ar")
        .env("CARGO_PROFILE_RELEASE_DEBUG", "false")
        .env("CARGO_PROFILE_RELEASE_STRIP", "symbols")
        // Nix's compiler flags name the MinGW headers and libraries: the target gets the library
        // folders; native build scripts must not see them.
        .env_remove("NIX_CFLAGS_COMPILE")
        .env_remove("NIX_LDFLAGS");
    let libs: Vec<String> =
        std::env::var("NIX_LDFLAGS_FOR_TARGET").unwrap_or_default().split_whitespace().filter(|f| f.starts_with("-L")).map(str::to_owned).collect();
    if !libs.is_empty() {
        c.env("CARGO_TARGET_X86_64_PC_WINDOWS_GNU_RUSTFLAGS", libs.join(" "));
    }
    // Some build scripts call `windres` without MinGW's prefix.
    if on_path("windres").is_none() {
        let windres = on_path(WINDRES).ok_or(format!("installer-windows: {WINDRES} not on PATH"))?;
        let shim = target_dir.join("installer-windows-bin");
        std::fs::create_dir_all(&shim).map_err(|e| format!("{}: {e}", shim.display()))?;
        let link = shim.join("windres");
        let _ = std::fs::remove_file(&link);
        link_to(&windres, &link)?;
        let path = std::env::var_os("PATH").unwrap_or_default();
        let joined = std::env::join_paths(std::iter::once(shim).chain(std::env::split_paths(&path))).map_err(|e| e.to_string())?;
        c.env("PATH", joined);
    }
    c.current_dir(root);
    crate::run(c, "cargo build --release --target x86_64-pc-windows-gnu")
}

#[cfg(unix)]
fn link_to(from: &Path, link: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(from, link).map_err(|e| format!("{}: {e}", link.display()))
}

#[cfg(not(unix))]
fn link_to(from: &Path, link: &Path) -> Result<(), String> {
    std::fs::copy(from, link).map(|_| ()).map_err(|e| format!("{}: {e}", link.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The checked-in icon is the one the code draws: run `cargo xtask icon` after changing it.
    #[test]
    fn the_icon_file_is_current() {
        let on_disk = std::fs::read(crate::root().join(ICON)).unwrap();
        assert!(on_disk == emcraft_render::icon::ico().unwrap(), "{ICON} is stale: run `cargo xtask icon`");
    }

    #[test]
    fn versions_and_licence() {
        assert_eq!(file_version("0.1.0").as_deref(), Some("0.1.0.0"));
        assert_eq!(file_version("1.22.3-beta.1+abc").as_deref(), Some("1.22.3.0"));
        assert_eq!(file_version("1.2"), None);
        assert_eq!(file_version("1.2.x"), None);
        assert_eq!(file_version("1.2.99999"), None);
        let text = licence_text(&crate::root()).unwrap();
        assert!(text.contains("MIT License") && text.contains("Apache License"));
        assert!(!text.replace("\r\n", "").contains('\n'));
        assert_eq!(sh_quote("a'b"), "'a'\\''b'");
    }
}

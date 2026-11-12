//! The control channel's per-launch token (docs/control-protocol.md), adapted from SolidCraft's
//! `crates/engine/src/control_auth.rs`: `emcraft --control PORT` makes a random token, writes it
//! to a file only the user can read ([`token_path`]) and refuses every connection whose first
//! request doesn't carry it, so other users on the same machine can't drive the app.

use std::io::{Error, ErrorKind, Result, Write};
use std::path::{Path, PathBuf};

/// Shortest token accepted from `EMCRAFT_CONTROL_TOKEN` (hex characters).
pub const MIN_TOKEN_LEN: usize = 32;

/// The user's data folder for EMCraft.
fn data_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA").map(|d| PathBuf::from(d).join("EMCraft"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/EMCraft"))
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        std::env::var_os("XDG_DATA_HOME")
            .filter(|d| !d.is_empty())
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
            .map(|d| d.join("emcraft"))
    }
}

/// The token file of the app listening on `port`: `<data folder>/control/<port>.token`
/// (`EMCRAFT_CONTROL_DIR` replaces the folder).
pub fn token_path(port: u16) -> Option<PathBuf> {
    let dir = match std::env::var_os("EMCRAFT_CONTROL_DIR").filter(|d| !d.is_empty()) {
        Some(d) => PathBuf::from(d),
        None => data_dir()?.join("control"),
    };
    Some(dir.join(format!("{port}.token")))
}

/// A token set on purpose with `EMCRAFT_CONTROL_TOKEN` (test harnesses), if it is long enough.
pub fn env_token() -> Option<String> {
    std::env::var("EMCRAFT_CONTROL_TOKEN").ok().map(|t| t.trim().to_owned()).filter(|t| t.len() >= MIN_TOKEN_LEN)
}

/// A new random token: 32 bytes from the operating system, as hex.
pub fn new_token() -> Option<String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).ok()?;
    Some(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

/// `a == b`, taking the same time wherever they differ.
pub fn tokens_match(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// Writes `token` to `path`, readable and writable only by the user: the folder `0700` and the
/// file `0600` on Unix; on Windows the file's inherited permissions are replaced by one entry for
/// the user (`icacls`) before the token goes in. A file left by an earlier launch is replaced.
pub fn write_token_file(path: &Path, token: &str) -> Result<()> {
    let dir = path.parent().ok_or_else(|| Error::new(ErrorKind::InvalidInput, "no folder"))?;
    std::fs::create_dir_all(dir)?;
    let _ = std::fs::remove_file(path);
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
        opts.mode(0o600);
    }
    let mut f = opts.open(path)?;
    #[cfg(windows)]
    restrict_to_user(path)?;
    f.write_all(token.as_bytes())?;
    f.write_all(b"\n")?;
    f.sync_all()
}

/// Windows: drop inherited entries and grant only the current user full access.
#[cfg(windows)]
fn restrict_to_user(path: &Path) -> Result<()> {
    use std::os::windows::process::CommandExt;
    let user = std::env::var("USERNAME").map_err(|_| Error::other("USERNAME is not set"))?;
    let domain = std::env::var("USERDOMAIN").ok().filter(|d| !d.is_empty());
    let who = domain.map_or(user.clone(), |d| format!("{d}\\{user}"));
    let status = std::process::Command::new("icacls")
        .arg(path)
        .args(["/inheritance:r", "/grant:r", &format!("{who}:F")])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .creation_flags(0x0800_0000)
        .status()?;
    if status.success() { Ok(()) } else { Err(Error::other(format!("icacls failed to restrict {}", path.display()))) }
}

/// Removes the token file when the app ends.
pub struct TokenFile(pub PathBuf);

impl Drop for TokenFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_compare_whole() {
        assert!(tokens_match("abc123", "abc123"));
        assert!(!tokens_match("abc123", "abc124"));
        assert!(!tokens_match("abc123", "abc12"));
        assert!(!tokens_match("", "a"));
        let t = new_token().unwrap();
        assert_eq!(t.len(), 64);
        assert_ne!(t, new_token().unwrap());
    }

    /// The token file is the user's alone, and goes when the app ends.
    #[cfg(unix)]
    #[test]
    fn the_token_file_is_private_to_the_user() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("emcraft-control-auth-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("control/7878.token");
        std::fs::create_dir_all(dir.join("control")).unwrap();
        std::fs::write(&path, "stale").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        write_token_file(&path, "0123456789abcdef0123456789abcdef").unwrap();
        let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        assert_eq!(std::fs::read_to_string(&path).unwrap().trim(), "0123456789abcdef0123456789abcdef");
        drop(TokenFile(path.clone()));
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

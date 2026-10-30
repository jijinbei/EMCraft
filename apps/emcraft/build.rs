//! Windows: the app icon and version go into the executable (`assets/emcraft.ico`, written by
//! `cargo xtask icon`), so Explorer, the Start menu and `.emcraft` files show them. Other targets:
//! nothing. A missing resource compiler costs the icon, not the build.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

fn main() {
    println!("cargo:rerun-if-changed=assets/emcraft.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon("assets/emcraft.ico")
        .set("ProductName", "EMCraft")
        .set("FileDescription", "EMCraft")
        .set("LegalCopyright", "EMCraft contributors, MIT OR Apache-2.0");
    if let Err(e) = res.compile() {
        println!("cargo:warning=emcraft.exe built without its icon: {e}");
    }
}

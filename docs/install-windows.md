# EMCraft on Windows

EMCraft ships for Windows 10 and 11 (64-bit) as one installer, `EMCraft-VERSION-setup.exe`.

## Download

Take `EMCraft-VERSION-setup.exe` from the repository's GitHub Releases. A build of any commit
is also an artifact of the **Windows installer** workflow (`.github/workflows/release-windows.yml`):
on GitHub run it from the Actions tab, or `gh workflow run release-windows.yml --ref BRANCH`.

## Install

Run the installer. It needs no administrator rights: it installs for your user only.

- **Unsigned installer.** The installer is not code-signed yet, so Windows SmartScreen may show
  "Windows protected your PC". Choose **More info → Run anyway**. Check the file first against
  the SHA-256 published with the release (`Get-FileHash EMCraft-*-setup.exe` in PowerShell).
- Pages: welcome, licence (MIT OR Apache-2.0, at your option), folder, install, finish (with
  "Start EMCraft").
- Silent install: `EMCraft-VERSION-setup.exe /S` (and `/D=C:\path` as the last argument for
  another folder).

What it puts where:

| What | Where |
|---|---|
| `emcraft.exe` (the app), `emcraft-cli.exe`, the licence, the uninstaller | `%LOCALAPPDATA%\Programs\EMCraft` |
| Start menu: **EMCraft**, **EMCraft Command Prompt** | `%APPDATA%\Microsoft\Windows\Start Menu\Programs` |
| `.emcraft` files open in EMCraft (`emcraft.exe "<file>"`) | `HKCU\Software\Classes\.emcraft`, `EMCraft.Project` |
| `emcraft` and `emcraft-cli` from the Run box (Win+R) | `HKCU\…\App Paths` |
| Apps & features entry | `HKCU\…\Uninstall\EMCraft` |

### The command line

`emcraft-cli` is **not added to PATH**: the installer leaves your PATH alone (an installer that
rewrites PATH can truncate a long one). Open **EMCraft Command Prompt** from the Start menu: a
terminal where `emcraft-cli` works. To have it everywhere, add the folder to your user PATH
yourself, in PowerShell:

```powershell
[Environment]::SetEnvironmentVariable("Path", [Environment]::GetEnvironmentVariable("Path", "User") + ";$env:LOCALAPPDATA\Programs\EMCraft", "User")
```

`emcraft.exe` is a window program: it opens no console, and what it prints (`--help`,
`--version`, errors) shows only when its output is redirected
(`emcraft --version | Out-String`). Use `emcraft-cli` for terminal work.

## Where settings live

| What | Where |
|---|---|
| Preferences, recent files | `%APPDATA%\emcraft\prefs.json` (`EMCRAFT_CONFIG_DIR` replaces the folder) |
| Your themes | `%APPDATA%\emcraft\themes` |
| The control channel's token (`emcraft --control PORT`) | `%LOCALAPPDATA%\EMCraft\control\PORT.token`, readable by your user only (`EMCRAFT_CONTROL_DIR` replaces the folder) |
| Scratch files of large factorizations | `%TEMP%` (`EMCRAFT_SCRATCH` replaces it) |

The factorization keeps up to 8 GB in memory on Windows before it spills to scratch files;
`EMCRAFT_FACTOR_MEMORY_GB` sets another limit.

## Uninstall

**Settings → Apps → Installed apps → EMCraft → Uninstall**, or run
`%LOCALAPPDATA%\Programs\EMCraft\uninstall.exe` (`/S` for silent). It removes the program, the
shortcuts and the `.emcraft` association. Your projects and your settings in `%APPDATA%\emcraft`
are kept; delete that folder to remove them too.

## What the Windows build leaves out

The installer's build carries no GPU kernels: CubeCL's CPU and CUDA runtimes link a prebuilt
LLVM that has no build for the toolchain the installer is made with (MinGW-w64). The finite
element and time-domain solvers run on the CPU as everywhere; `solve.time_domain` on a GPU
backend reports that the backend is not built.

## Building the installer on Linux

One command, from the repository:

```
cargo xtask installer-windows [--out FILE]
```

It writes `target/EMCraft-VERSION-setup.exe` (or FILE). What it does:

1. Needs MinGW-w64's `x86_64-w64-mingw32-gcc` and NSIS's `makensis` on PATH; when either is
   missing and Nix is installed, it runs itself again inside
   `nix-shell -p pkgsCross.mingwW64.buildPackages.gcc pkgsCross.mingwW64.windows.pthreads nsis`.
2. `rustup target add x86_64-pc-windows-gnu`, then
   `cargo build --release --target x86_64-pc-windows-gnu -p emcraft -p emcraft-cli`, linked by
   MinGW's GCC (symbols stripped). The binaries need only Windows' own DLLs. `apps/emcraft/build.rs`
   puts the icon and version into `emcraft.exe` with MinGW's `windres`.
3. Stages the two executables, `apps/emcraft/assets/emcraft.ico` and a `LICENSE.txt` made from
   `LICENSE-MIT` and `LICENSE-APACHE` in `target/installer-windows`.
4. `makensis` packs them with `apps/emcraft/installer/emcraft.nsi`, the version from Cargo.toml.

The release workflow runs the same command on an Ubuntu runner, then installs the result on a
Windows runner, runs `emcraft-cli commands` and `emcraft --version`, checks the `.emcraft`
association and uninstalls.

The app icon is drawn by code (`emcraft_render::icon`); after changing it, `cargo xtask icon`
rewrites `apps/emcraft/assets/emcraft.ico` (a test fails while the file is stale).

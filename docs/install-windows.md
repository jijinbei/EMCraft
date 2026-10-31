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

The installer's build carries no GPU kernels: CubeCL's CUDA runtime links a prebuilt LLVM that
has no build for the toolchain the installer is made with (MinGW-w64), and no released build
carries CubeCL's CPU runtime (`cubecl-cpu`, for the tests). The finite element and time-domain
solvers run on the CPU as everywhere; `solve.time_domain` on a GPU backend reports that the
backend is not built.

What is in: the whole engine (every command, the finite element and time-domain solvers, the
importers, SolidCraft designs through `solidcraft-exchange`), the desktop app drawn with OpenGL
(eframe's wgpu renderer — DirectX 12, Vulkan or OpenGL — with glow, OpenGL 2, as its fallback; docs/adr/0004-gpu-viewport.md), the system's file dialogs. What is out: CUDA and CubeCL's CPU runtime.

The window needs an OpenGL 2 driver. Windows' own fallback (OpenGL 1.1, as in a virtual machine
without a graphics driver, or some remote desktops) is not enough: the app then shows a message
box ("EMCraft could not open its window") and exits; `emcraft-cli` works there all the same.

Japanese text in the window is drawn in the system's Japanese font (Yu Gothic UI, else Yu
Gothic, Meiryo UI, Meiryo, MS UI Gothic, MS Gothic), or in the embedded subset of Noto Sans JP
when none is found (docs/languages.md).

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
Windows runner, runs `emcraft-cli commands`, a small eigenmode script (`emcraft-cli run`), the
high-pass sample (`emcraft-cli sample highpass --out DIR`) and `emcraft --version`, checks the
`.emcraft` association and uninstalls. It runs on a `v*` tag (and then attaches the installer to
that release) or by hand (`workflow_dispatch`: the installer is the run's
`emcraft-windows-installer` artifact).

## How the Windows build is checked

| Check | How | Last result (2026-10-31, 0.1.0) |
|---|---|---|
| Cross build, lints | `cargo xtask installer-windows`; `cargo clippy --release --target x86_64-pc-windows-gnu -p emcraft -p emcraft-cli -- -D warnings` with MinGW's GCC as `CC` | builds (2026-11-01, with wgpu: DirectX 12, Vulkan and GL backends); installer 25.3 MB (`emcraft.exe` 47.7 MB, `emcraft-cli.exe` 34.5 MB) |
| CLI | Wine 10 on Linux (Nix's `wineWow64Packages.stable`): `emcraft-cli commands`, `run` on an eigenmode script, `sample highpass --out DIR` | all run; the sample's worst \|S − S_exact\| = 1.0e-3, its pictures written |
| Installer | Wine: `setup.exe /S`, the `.emcraft` association in the registry, `uninstall.exe /S` | installs to `%LOCALAPPDATA%\Programs\EMCraft`, association set, uninstall removes both |
| Window | Wine 10 on Xvfb (2026-11-01): `emcraft --sample highpass --control PORT`, then `ui.inspect`, `ui.render` and `ui.screenshot` over the control channel; Vulkan through Nix's loader and Mesa's lavapipe (`VK_ICD_FILENAMES`, `LD_LIBRARY_PATH`), Mono and Gecko off (`WINEDLLOVERRIDES=mscoree,mshtml=`) | with lavapipe: the window opens on wgpu, the 3D view drawn by its GPU (`view3d.renderer: gpu`, adapter `llvmpipe (Vulkan)`), the screenshot and `ui.render` alike; without Vulkan: wgpu finds no surface and the app tries OpenGL (`the window cannot draw with wgpu …; trying OpenGL`), which this Xvfb lacks too, so it reports and exits |
| Install and run on Windows | the release workflow's `smoke` job on `windows-latest` | each tagged or hand-started run |

What only a real Windows machine shows: SmartScreen and code signing; the window on real graphics
drivers (DirectX 12, Vulkan or OpenGL on Intel, AMD, NVIDIA; WARP; HiDPI scaling; a remote desktop); the system's file
dialogs and fonts as Windows draws them; opening a `.emcraft` file from Explorer; GPU kernels
(not in this build).

The app icon is drawn by code (`emcraft_render::icon`); after changing it, `cargo xtask icon`
rewrites `apps/emcraft/assets/emcraft.ico` (a test fails while the file is stale).

# Development workflow

## Windows prerequisites and location

Use `D:\CodeSpace\track-mesh`, Rust for Windows, and Visual Studio C++ build tools. SQLite is bundled. Run the build commands from an x64 Visual Studio developer shell so the MSVC compiler/linker and Windows SDK header/library paths are initialized. Private device/project material uses current-user Windows DPAPI; a sandbox account may be unable to decrypt another account's data. There is no plaintext-key fallback.

The recorded successful builds used Rust 1.98.1. The workspace declares `rust-version = "1.88"` because igd-next 0.18 requires 1.88; the Desktop crate declares 1.90. These minimum versions have not been separately verified. Use the recorded working toolchain unless the task is explicitly minimum-version compatibility work.

The Windows Desktop requires WebView2 and a supported Node/npm runtime. Vite 8 supports Node `^20.19.0 || >=22.12.0`; use Node 22.12 or later for this workflow. Under Node 22.11.0, `npm ci` skips Vite's native Windows binding and `vite build` fails with "Cannot find native binding". This host now uses Node 26.12.1 with npm 12.2.0. Foundation phase 4 uses React/TypeScript/Vite and bundled static assets. Before checking the complete workspace, run `npm ci`, `npm run typecheck`, and `npm run build` in `apps/desktop`; no development server/watcher is needed. See [PHASE4.md](PHASE4.md) for historical native operation. Core-only CLI/service builds do not need frontend assets.

## Build and static checks

Run from repository root:

```powershell
cargo fmt --all -- --check
cargo check --workspace --locked --offline
cargo clippy --workspace --lib --bins --locked --offline -- -D warnings
cargo build -p trackmesh-core --locked --offline
cargo build -p trackmesh-desktop --locked --offline
target\debug\trackmesh-agent.exe --help
target\debug\trackmesh-desktop.exe --help
```

GitHub Actions runs the frontend typecheck and build, `cargo fmt --check` and the workspace `cargo clippy -D warnings` on every push to `main` and every pull request (`.github/workflows/check.yml`, Windows, read-only token, no secrets). It does not build the VST3 or the installer.

`--offline` works when the locked dependencies are already cached. On a new machine, omit it for the initial dependency download while retaining `--locked`. For an optimized binary use `cargo build -p trackmesh-core --release --locked`; the release profile enables thin LTO and one codegen unit. No benchmark claim follows from those settings.

Do not add/run automated tests or benchmarks for this MVP. Formatting, compilation, static analysis, code inspection, and explicitly authorized manual operation provide the current checks. Report actual evidence and limitations. Historical loopback checks in `IMPLEMENTATION.md` do not establish real NAT traversal or validate a later refactor.

## Release build and Windows installer

From an x64 Visual Studio developer shell, start with a clean, committed Desktop manifest and the pinned VST3 SDK in `vendor/vst3sdk` (see [PHASE5.md](PHASE5.md)). The order is Rust executables, then the VST3 bundle that copies them, then the installer that bundles `.build/vst3/VST3/Release/TrackMesh.vst3`. It works from a fresh clone:

```powershell
cargo build -p trackmesh-core --release --locked
cd apps/desktop
npm ci
npm run typecheck
npx tauri build --no-bundle
cd ../..
cmake -S plugins/vst3 -B .build/vst3 -G Ninja -DCMAKE_BUILD_TYPE=Release -DTRACKMESH_AGENT=target/release/trackmesh-agent.exe -DTRACKMESH_DESKTOP=target/release/trackmesh-desktop.exe
cmake --build .build/vst3 --target TrackMesh
cd apps/desktop
npm run installer
cd ../..
git diff -- apps/desktop/src-tauri/Cargo.toml
git checkout -- apps/desktop/src-tauri/Cargo.toml
```

`npm run installer` is `tauri build --config src-tauri/tauri.installer.conf.json`. Only that file lists the VST3 bundle as installer resources, so `cargo build`, `npx tauri dev` and `npx tauri build --no-bundle` do not need `.build/vst3`. A plain `npx tauri build` stops before bundling with a message instead of producing an installer without the plugin.

The installer is `target/release/bundle/nsis/TrackMesh_<version>_x64-setup.exe`. It is per-machine (UAC prompt): the app goes to `Program Files\TrackMesh`, the plugin to `Common Files\VST3\TrackMesh.vst3` (`apps/desktop/src-tauri/installer-hooks.nsh`), and a missing WebView2 runtime is downloaded by the Tauri bootstrapper. The same hooks replace every inbound Windows Firewall rule for the installed `trackmesh-desktop.exe`, including a block created by a dismissed Windows prompt, with two allow rules named `TrackMesh` (TCP and UDP 43101, all profiles, program-scoped). Router ports are still opened only by consented UPnP. Uninstall removes the app, the plugin and those firewall rules; user data in `%LOCALAPPDATA%\TrackMesh`, project folders and backups stay. The plugin first looks for `trackmesh-desktop.exe` next to its DLL, then in `Program Files\TrackMesh`. The installer is unsigned, so Windows may show an unrecognized-app warning or block the installation, depending on its settings and policy. `npx tauri build --no-bundle` produces only `target/release/trackmesh-desktop.exe`. The Tauri configuration runs the frontend build and uses its bundled assets. Restore a Tauri CLI rewrite of `apps/desktop/src-tauri/Cargo.toml` after building; never commit that generated change. The #43 documentation work did not run a release build or inspect the exact manifest rewrite. No current trial package is published; create a new package from reviewed `master` when needed.

A public release must not embed a personal Supabase project. `TRACKMESH_DEFAULT_SUPABASE_URL` and `TRACKMESH_DEFAULT_SUPABASE_PUBLISHABLE_KEY` are for private builds only. Supply shared service settings through the native app for a public build; keep actual values out of source, documents and distribution instructions. The private values come from the gitignored `.cargo/config.toml` `[env]` table, so build public installers from a fresh clone of the public repository (or a shell where neither variable is set and no such file exists); never open, print or commit that file. After a public build, Advanced → Default invite service must show "Not set" on a fresh install. Updater signing keys stay with the maintainer, outside the repository, CI logs and agent context; only the public key goes into the Tauri configuration.

## Data and manual operation

Foundation phase 5 VST3 builds separately through CMake/Ninja with the pinned official SDK. Build Rust core/desktop and static frontend first; follow [PHASE5.md](PHASE5.md). CMake disables validator execution and installed-folder links. Build only requested plugin/manual editor-host targets; do not run SDK test suites or benchmarks. On this host, Ninja avoids inherited duplicate `Path`/`PATH` entries that made MSBuild reject its compiler launch; no machine-wide environment change is required.

- Default data: `%LOCALAPPDATA%\TrackMesh`; override with `--data-dir` or `TRACKMESH_DATA_DIR`.
- Local storage: `trackmesh.db`, `chunks`, protected `secrets`, and per-project lock files. Keep it separate from DAW project folders.
- Each simulated device needs a separate data directory and working folder. Fixtures/logs/build outputs belong in ignored scratch paths, never beside production samples.
- Invitations contain a shared project key. Exchange them privately; never print them into logs or commit them.
- Receive and recover require closing the DAW project. Preserve `.trackmesh-<project>-<transfer>` staging/backup data if recovery is pending.
- Logs go to stderr; CLI JSON goes to stdout. For transfer details use `$env:RUST_LOG='trackmesh_transfer=info,trackmesh_network=info'`.
- Follow [current setup](setup/README.md) for the Desktop invite-link/request/Approve flow and [PHASE2.md](PHASE2.md) for Advanced CLI commands and explicitly chosen legacy cards. The CLI has no invite-link flow; `serve` alone cannot open/approve enrollment windows. Safe `status` supplies public project/sender UUIDs; never read invitation payloads into agent context. Do not install a permanent daemon or introduce file watchers while debugging.
- [PHASE3.md](PHASE3.md) describes the foundation mailbox and relay/rendezvous code. Both are now outside the build in `future/` and deferred to post-1.0. Keep service and simulated-device data in separate directories.

## Local phase branches

Commit the current completed work before beginning a phase. For each new phase, create a local `codex/phase-N-description` branch from the integration branch, implement and validate it, then commit and merge it locally into `master` with `--no-ff`. Push the resulting `master` normally; no force push or remote branch/PR is required. This is the user's requested Git workflow, not automatic merging of divergent project snapshots.

## Before editing

1. Read root `AGENTS.md` and the owning crate's instructions.
2. Use `CODEMAP.md` to inspect the narrowest relevant path and its callers.
3. Identify affected compatibility contracts: signed bytes, wire frames, schema/state labels, paths, journal, API/CLI fields, or transaction ordering.
4. Preserve unrelated working changes. Update maps/docs with module/API moves and record the checks actually run.

When filesystem permissions prevent editing the canonical repository, request the correct workspace/writable root. Preparing a reviewable staging copy is possible, but do not claim the canonical repository was updated or bypass the permission boundary.

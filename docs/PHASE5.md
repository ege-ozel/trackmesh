# Phase 5 Windows VST3 and IPC operation guide

TrackMesh's native C++ VST3 is a small status/control adapter. Rust core owns snapshots, publication, storage and networking. Current progress and actual validation evidence are in [IMPLEMENTATION.md](IMPLEMENTATION.md).

## Build

Use Windows x64, the x64 MSVC developer environment, Windows SDK, CMake 3.25+ and Ninja. Build the desktop's static frontend as described in [PHASE4.md](PHASE4.md). The reference SDK is official Steinberg `v3.8.0_build_66`, root commit `9fad9770f2ae8542ab1a548a68c1ad1ac690abe0`, with its pinned submodules. SDK sources and their licenses remain in ignored `vendor`; they are not copied into repository source.

```powershell
git clone --depth 1 --branch v3.8.0_build_66 --recurse-submodules --shallow-submodules https://github.com/steinbergmedia/vst3sdk.git vendor/vst3sdk
cargo build -p trackmesh-core -p trackmesh-desktop --locked --offline
cmake -S plugins/vst3 -B .build/vst3 -G Ninja -DCMAKE_BUILD_TYPE=Release
cmake --build .build/vst3 --target TrackMesh --parallel 4
```

The output is `.build/vst3/VST3/Release/TrackMesh.vst3`: a **directory bundle** with `Contents/x86_64-win/TrackMesh.vst3`, bundled desktop/CLI executables and SDK-generated `Contents/Resources/moduleinfo.json`. Keep the entire bundle together. Desktop needs WebView2; C++ needs the MSVC runtime. CMake defaults to the built debug Rust executables; for a release bundle build Rust with `--release` and set `TRACKMESH_AGENT` and `TRACKMESH_DESKTOP` to their release paths. Every plugin-target build refreshes changed Rust executables even when C++ sources need no recompilation. Close applications using this bundle before rebuilding because Windows locks loaded binaries. `TRACKMESH_VST3_SDK` accepts another local copy of the pinned SDK.

CMake disables SDK validator execution and installed-directory links. No tests or benchmarks run. Optional `-DTRACKMESH_BUILD_EDITOR_HOST=ON` exposes the SDK editor host; build only `TrackMesh` and `editorhost`, then open the bundle in `bin/editorhost.exe` for manual inspection. This does not establish DAW compatibility or audio validation.

## Project and controls

The bundle retains the SDK's original license notice in `Contents/Resources/Steinberg-SDK-LICENSE.txt`.

1. Load TrackMesh as a VST3 effect. It transparently passes mono/stereo 32-bit or 64-bit audio with zero latency. `process` performs no allocation, lock, IPC, filesystem or network work.
2. The selected linked-project UUID lives in the existing versioned 24-byte component state: format marker/version and 16 UUID bytes. Saving/reopening the same DAW project restores that UUID. Invalid, nil or truncated state is rejected before changing the current ID. Class ID `AD81AE224F924B139E7A621C14B409D8` is permanent.
3. Without a plugin pipe, the worker starts the bundled desktop, which owns the Rust agent and single peer runtime. An unlinked UUID shows a bounded project picker: choose a project already linked or imported in TrackMesh. Duplicate displayed names include their last folder component. **Change project** corrects a wrong pick. Selection marks component state dirty through the SDK's UI-thread `IComponentHandler2::setDirty`; a host lacking/rejecting it cannot change the selection. Existing saved UUIDs that are already linked attach directly. No automatic `.als`/`.flp` path discovery is used. Follow [current automatic pairing setup](setup/README.md); private invitations stay outside plugin/agent context.
4. **Share current version** settles project files, then uses the same Engine send implementation as Desktop Share: local snapshot commit and direct version notice, with head hints and queued membership actions handled by the same session's Supabase worker. Supabase projects skip the legacy mailbox. The view uses collaborator names and Rust-derived last-known status lines; IDs remain small muted details. Last-known history is not fresh verified equality.
5. **Get collaborator's version** is available when a known source is online and history is not known divergent or installing/recovering. It reopens the same Desktop window, focuses the project's card and asks the user to save/close the DAW project before choosing Get there; it never starts a receive. An unbound plugin offers **Open TrackMesh** to link a folder first. Collaborator selection, fresh verification, transfer and recovery stay in Desktop/core.

The SDK `SingleComponentEffect` combines the small controller/processor to keep one UUID/session owner independent of editor lifetime. Separate components were considered: they require extra UUID/lifetime messaging without moving business logic out of Rust. This non-distributable Windows surface uses native HWND `CPluginView`, with no VSTGUI/additional GUI framework. Editor close/reopen is not component session end.

## IPC and lifetime

Endpoint: `\\.\pipe\TrackMesh-v1-<Windows-user-SID>-<suffix>`, empty suffix by default. Protected current-user-only DACL, remote-client rejection and exclusive first-instance ownership. Each frame is an unsigned 32-bit **little-endian byte length**, then UTF-8 JSON, bounded to 64 KiB. Existing `PluginEvent` names/fields are unchanged. Defaulted additive `PluginStatus` fields carry at most 64 project names/folder components, presentation/action capabilities and the eligible mailbox pending count. Old plugins ignore these fields; new plugins connected to an old agent show an upgrade notice without sending unsupported events or fields.

Connect with `PLUGIN_CONNECTED` and a non-nil session/project pair. `PROJECT_ID` rebinds the same session after state restore; all other events must match. Duplicate live sessions, malformed/oversized frames and mismatches disconnect that client. Limits: 16 clients, eight simultaneous snapshot workers, one snapshot job per project, 64 remembered project messages, eight recent snapshots per peer, 64 ancestry steps, bounded frame/write timeouts and polling rate. Blocking summaries/snapshots run outside the listener and network actor.

`getState` coalesces `SAVE_OBSERVED`: serialization observation includes host probing/autosaves and does not establish completed DAW disk writing. Per-project **Share automatically when the DAW project is saved** defaults off, including existing projects. Off updates only the save notice; explicit Share still settles and shares. On uses a project debounce and bounded inventory/writer settlement, then the existing guarded snapshot/commit/share workflow. The plugin checkbox and **Settings → DAW plugin** change the same atomic local setting. A new SET_AUTO_SHARE pipe event is sent only when auto_share_supported is advertised; old agents hide that checkbox, old plugins' saves obey the new preference. No folder watcher runs. Unchanged contents reuse their snapshot; a failure never invents a completed snapshot. Details and actual evidence are in [IMPLEMENTATION.md](IMPLEMENTATION.md). Termination queues `SESSION_ENDING` without awaiting pipe I/O. A worker-held DLL reference protects code/state until bounded I/O and C++ cleanup finish. Signals carry their UUID generation so old button actions cannot target a restored project.

The bundled desktop launches with `--plugin-owner` and has the native tray even while its window is hidden. Plugin-only idle owners retain the existing work-based 60-second grace. A plugin Desktop handoff or explicit launch reopens the same window; window X retains it in the tray. **Close safely** and tray **Quit** use the confirmed availability-loss flow when collaborators exist, stop accepting new plugin work and drain accepted save/share jobs before metadata/network shutdown. Repeated plugin launches do not hold an explicit window session. After a successful connection, the plugin does not repeatedly launch an owner that was interrupted; reopen Desktop to reconnect.

The pinned plugin worker drains queued current-generation save/send signals before `SESSION_ENDING`, with a 30-second ending retry budget checked between bounded requests. Failed transport restores the signal for retry without replacing newer-generation work. Host termination still returns without joining IPC. Forced host process exit can kill the worker before delivery; it cannot guarantee delivery of an event the agent never received. Actual DAW shutdown/save behavior remains unverified. Save settlement's two-second quiet period and Windows writer checks cannot prove completion of delayed future host writes.

Explicit headless bridge:

```powershell
target\debug\trackmesh-agent.exe plugin-serve
```

Existing network/mailbox options plus `--ipc-suffix` are accepted. Exit follows 60 seconds without bound sessions, accepted snapshots, active peer responses or authored pending publication, or explicit Ctrl+C. Ctrl+C awaits accepted local work; pending metadata remains durable for restart. An unavailable or missing mailbox can retain a pending owner indefinitely; ACK is required to clear pending work. This is work-based retention, with no installed permanent service. The headless bridge owns the data-directory runtime lease: close plugin sessions and let it exit before opening desktop against that profile. Ordinary CLI `serve` does not expose IPC.

In development builds (no `NDEBUG`), `TRACKMESH_AGENT_PATH` selects an absolute `trackmesh-desktop.exe` or `trackmesh-agent.exe` path; release plugins ignore it and start only the app next to the plugin or the installed `Program Files\TrackMesh\trackmesh-desktop.exe`; no shell/PATH search or project-directory executable is selected automatically. `TRACKMESH_DATA_DIR` retains the data profile contract. Separate/disposable profiles need matching `TRACKMESH_IPC_SUFFIX` in DAW, desktop and headless bridge (up to 32 ASCII letters/digits/hyphens). Suffix is routing, not authentication. Never start another swarm against the same directory.

References: [official SDK](https://github.com/steinbergmedia/vst3sdk/tree/v3.8.0_build_66), [SingleComponentEffect](https://steinbergmedia.github.io/vst3_doc/vstsdk/classSteinberg_1_1Vst_1_1SingleComponentEffect.html), [Microsoft named-pipe security](https://learn.microsoft.com/en-us/windows/win32/ipc/named-pipe-security-and-access-rights).

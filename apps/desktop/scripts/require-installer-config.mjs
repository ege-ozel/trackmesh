// Runs before bundling unless tauri.installer.conf.json is merged, so a plain
// `tauri build` cannot produce an installer without the VST3 plugin.
console.error("Build the installer with `npm run installer`: it adds the VST3 plugin from .build/vst3.");
process.exit(1);

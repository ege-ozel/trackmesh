//! Capability-bearing documents never cross the WebView boundary.
use anyhow::{ensure, Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    path::Path,
};
use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;
use zeroize::Zeroizing;

pub fn read_document<T: DeserializeOwned>(app: &AppHandle) -> Result<T> {
    let path = app
        .dialog()
        .file()
        .add_filter("TrackMesh JSON", &["json"])
        .blocking_pick_file()
        .context("File selection cancelled")?
        .into_path()?;
    let mut data = Zeroizing::new(Vec::new());
    std::fs::File::open(path)?
        .take(64 * 1024 + 1)
        .read_to_end(&mut data)?;
    ensure!(data.len() <= 64 * 1024, "Pairing document exceeds 64 KiB");
    // A parser error may quote an input value; never forward it for invitation documents.
    serde_json::from_slice(&data).map_err(|_| anyhow::anyhow!("Invalid TrackMesh JSON document"))
}

pub fn write_document(app: &AppHandle, name: &str, document: &impl Serialize) -> Result<String> {
    let path = app
        .dialog()
        .file()
        .add_filter("TrackMesh JSON", &["json"])
        .set_file_name(name)
        .blocking_save_file()
        .context("File selection cancelled")?
        .into_path()?;
    ensure!(
        path.extension()
            .is_some_and(|v| v.eq_ignore_ascii_case("json")),
        "Choose a .json filename"
    );
    write_new(&path, document)?;
    Ok(path.display().to_string())
}

fn write_new(path: &Path, document: &impl Serialize) -> Result<()> {
    let data = serde_json::to_vec_pretty(document)?;
    ensure!(data.len() <= 64 * 1024, "Pairing document exceeds 64 KiB");
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .context("Choose a new filename; existing files are preserved")?;
    file.write_all(&data)?;
    file.sync_all()?;
    Ok(())
}

pub fn choose_folder(app: &AppHandle) -> Result<Option<String>> {
    app.dialog()
        .file()
        .blocking_pick_folder()
        .map(|path| Ok(display_path(&path.into_path()?)))
        .transpose()
}

pub fn default_projects_location() -> Result<std::path::PathBuf> {
    use std::{ffi::OsString, os::windows::ffi::OsStringExt, ptr};
    use windows_sys::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_Documents, SHGetKnownFolderPath},
    };
    let mut raw = ptr::null_mut();
    let result = unsafe { SHGetKnownFolderPath(&FOLDERID_Documents, 0, ptr::null_mut(), &mut raw) };
    ensure!(
        result >= 0 && !raw.is_null(),
        "Windows Documents folder is unavailable"
    );
    let path = unsafe {
        let mut len = 0;
        while *raw.add(len) != 0 {
            len += 1;
        }
        let value = OsString::from_wide(std::slice::from_raw_parts(raw, len));
        CoTaskMemFree(raw.cast());
        std::path::PathBuf::from(value).join("TrackMesh")
    };
    Ok(path)
}

pub fn display_path(path: &std::path::Path) -> String {
    let display = path.display().to_string();
    display.strip_prefix(r"\\?\").unwrap_or(&display).to_owned()
}

pub fn open_in_explorer(path: &Path) -> Result<()> {
    use std::{iter, os::windows::ffi::OsStrExt};
    use windows_sys::Win32::UI::Shell::ShellExecuteW;

    ensure!(path.is_dir(), "Folder is not available");
    // The shell does not accept verbatim `\\?\C:\` paths; drive paths are safe to shorten.
    let raw = path.as_os_str().to_string_lossy();
    let shortened = raw
        .strip_prefix(r"\\?\")
        .filter(|rest| rest.as_bytes().get(1) == Some(&b':'))
        .map(Path::new);
    let path = shortened.unwrap_or(path);
    let verb = "open"
        .encode_utf16()
        .chain(iter::once(0))
        .collect::<Vec<_>>();
    let target = path
        .as_os_str()
        .encode_wide()
        .chain(iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            1,
        )
    };
    ensure!(result as isize > 32, "Could not open folder in Explorer");
    Ok(())
}

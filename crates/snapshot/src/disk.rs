//! Free-space and file-kind questions asked before a receive writes anything.
use std::path::{Path, PathBuf};

/// Whether a project-relative file can run code when opened or loaded: programs, libraries,
/// plugins, installers, scripts and shortcuts. Used only to warn before receiving; DAW project
/// files that contain their own code (for example Max for Live devices) are not included.
pub fn is_executable_path(relative: &str) -> bool {
    let name = relative.rsplit('/').next().unwrap_or(relative);
    let Some((_, extension)) = name.rsplit_once('.') else {
        return false;
    };
    matches!(
        extension.to_ascii_lowercase().as_str(),
        "exe"
            | "dll"
            | "sys"
            | "drv"
            | "ocx"
            | "cpl"
            | "scr"
            | "com"
            | "pif"
            | "msi"
            | "msp"
            | "msix"
            | "appx"
            | "appxbundle"
            | "msixbundle"
            | "bat"
            | "cmd"
            | "ps1"
            | "psm1"
            | "psd1"
            | "vbs"
            | "vbe"
            | "js"
            | "jse"
            | "wsf"
            | "wsh"
            | "hta"
            | "lnk"
            | "url"
            | "reg"
            | "jar"
            | "chm"
            | "vst"
            | "vst3"
            | "aax"
            | "clap"
            | "application"
            | "appref-ms"
    )
}

/// The root of the volume holding `path` (for example `D:\`), or `None` when Windows can't tell.
pub fn volume_root(path: &Path) -> Option<PathBuf> {
    imp::volume_root(path)
}

/// Bytes this user may still write on the volume holding `path`.
pub fn free_bytes(path: &Path) -> Option<u64> {
    imp::free_bytes(path)
}

#[cfg(windows)]
mod imp {
    use std::{
        ffi::OsString,
        os::windows::ffi::{OsStrExt, OsStringExt},
        path::{Path, PathBuf},
    };
    use windows_sys::Win32::Storage::FileSystem::{GetDiskFreeSpaceExW, GetVolumePathNameW};

    fn wide(path: &Path) -> Vec<u16> {
        path.as_os_str().encode_wide().chain(Some(0)).collect()
    }

    pub fn volume_root(path: &Path) -> Option<PathBuf> {
        let input = wide(path);
        let mut buffer = vec![0u16; 1024];
        // SAFETY: `input` is NUL-terminated and `buffer` is writable for the length passed.
        let ok =
            unsafe { GetVolumePathNameW(input.as_ptr(), buffer.as_mut_ptr(), buffer.len() as u32) };
        if ok == 0 {
            return None;
        }
        let end = buffer.iter().position(|c| *c == 0).unwrap_or(buffer.len());
        Some(PathBuf::from(OsString::from_wide(&buffer[..end])))
    }

    pub fn free_bytes(path: &Path) -> Option<u64> {
        let input = wide(path);
        let mut available = 0u64;
        // SAFETY: `input` is NUL-terminated; the out pointer is a live u64 and the others may be null.
        let ok = unsafe {
            GetDiskFreeSpaceExW(
                input.as_ptr(),
                &mut available,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        (ok != 0).then_some(available)
    }
}

#[cfg(not(windows))]
mod imp {
    use std::path::{Path, PathBuf};
    pub fn volume_root(_path: &Path) -> Option<PathBuf> {
        None
    }
    pub fn free_bytes(_path: &Path) -> Option<u64> {
        None
    }
}

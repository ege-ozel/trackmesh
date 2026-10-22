//! Native invite-link clipboard handling with Windows history/cloud exclusion.
use anyhow::{ensure, Result};
use std::{ffi::c_void, ptr};
use windows_sys::Win32::System::{
    DataExchange::{
        CloseClipboard, EmptyClipboard, GetClipboardData, GetClipboardSequenceNumber,
        OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    },
    Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
};
use zeroize::Zeroizing;

const CF_UNICODETEXT: u32 = 13;

#[link(name = "kernel32")]
unsafe extern "system" {
    fn GlobalFree(memory: *mut c_void) -> *mut c_void;
}

pub fn copy_invite_link(link: &str) -> Result<u32> {
    let wide = Zeroizing::new(
        link.encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>(),
    );
    let text = unsafe {
        allocate_bytes(std::slice::from_raw_parts(
            wide.as_ptr().cast(),
            wide.len() * 2,
        ))?
    };
    let excluded = match unsafe { allocate_u32(1) } {
        Ok(handle) => handle,
        Err(error) => {
            unsafe {
                free_handle(text);
            }
            return Err(error);
        }
    };
    let no_history = match unsafe { allocate_u32(0) } {
        Ok(handle) => handle,
        Err(error) => {
            unsafe {
                free_handle(text);
                free_handle(excluded);
            }
            return Err(error);
        }
    };
    let no_cloud = match unsafe { allocate_u32(0) } {
        Ok(handle) => handle,
        Err(error) => {
            unsafe {
                free_handle(text);
                free_handle(excluded);
                free_handle(no_history);
            }
            return Err(error);
        }
    };
    let formats = [
        CF_UNICODETEXT,
        unsafe { format("ExcludeClipboardContentFromMonitorProcessing") },
        unsafe { format("CanIncludeInClipboardHistory") },
        unsafe { format("CanUploadToCloudClipboard") },
    ];
    let mut handles = [text, excluded, no_history, no_cloud];
    if formats.contains(&0) {
        unsafe {
            free_handles(&mut handles);
        }
        anyhow::bail!("Windows clipboard formats are unavailable");
    }
    let opened = unsafe { OpenClipboard(ptr::null_mut()) } != 0;
    if !opened {
        unsafe {
            free_handles(&mut handles);
        }
    }
    ensure!(opened, "Windows clipboard is unavailable");
    let success = unsafe {
        if EmptyClipboard() == 0 {
            false
        } else {
            formats
                .into_iter()
                .zip(handles.iter_mut())
                .all(|(format, handle)| {
                    let result = SetClipboardData(format, *handle as _);
                    if !result.is_null() {
                        *handle = ptr::null_mut();
                        true
                    } else {
                        false
                    }
                })
        }
    };
    if !success {
        unsafe {
            EmptyClipboard();
        }
    }
    let sequence = unsafe { GetClipboardSequenceNumber() };
    unsafe {
        CloseClipboard();
    }
    unsafe {
        free_handles(&mut handles);
    }
    ensure!(success, "Windows clipboard could not copy the invite link");
    Ok(sequence)
}

pub fn clear_if_matches(link: &str, sequence: u32) -> Result<()> {
    ensure!(
        unsafe { OpenClipboard(ptr::null_mut()) } != 0,
        "Windows clipboard is unavailable"
    );
    let matches = unsafe {
        if GetClipboardSequenceNumber() != sequence {
            false
        } else {
            let data = GetClipboardData(CF_UNICODETEXT);
            if data.is_null() {
                false
            } else {
                let memory = GlobalLock(data);
                if memory.is_null() {
                    false
                } else {
                    let size = global_size(data);
                    if !(2..=64 * 1024).contains(&size) || !size.is_multiple_of(2) {
                        GlobalUnlock(data);
                        false
                    } else {
                        let units = std::slice::from_raw_parts(memory.cast::<u16>(), size / 2);
                        let end = units
                            .iter()
                            .position(|unit| *unit == 0)
                            .unwrap_or(units.len());
                        let equal = units[..end].iter().copied().eq(link.encode_utf16());
                        GlobalUnlock(data);
                        equal
                    }
                }
            }
        }
    };
    if matches {
        unsafe {
            EmptyClipboard();
        }
    }
    unsafe {
        CloseClipboard();
    }
    Ok(())
}

/// Read and consume a valid invite link without exposing its payload to the webview.
pub fn take_invite_link() -> Result<crate::uri::ParsedInviteLink> {
    let fail = || anyhow::anyhow!("No TrackMesh invite link on the clipboard");
    if unsafe { OpenClipboard(ptr::null_mut()) } == 0 {
        return Err(fail());
    }
    let sequence = unsafe { GetClipboardSequenceNumber() };
    let parsed = unsafe {
        let data = GetClipboardData(CF_UNICODETEXT);
        if data.is_null() {
            Err(fail())
        } else {
            let memory = GlobalLock(data);
            if memory.is_null() {
                Err(fail())
            } else {
                let size = global_size(data);
                // A 512-byte ASCII URI occupies 1024 UTF-16 bytes, plus a possible terminator.
                let result = if !(2..=1026).contains(&size) || !size.is_multiple_of(2) {
                    Err(fail())
                } else {
                    let units = std::slice::from_raw_parts(memory.cast::<u16>(), size / 2);
                    let end = units
                        .iter()
                        .position(|unit| *unit == 0)
                        .unwrap_or(units.len());
                    let link = Zeroizing::new(String::from_utf16_lossy(&units[..end]));
                    crate::uri::parse_invite_link(link.trim()).map_err(|_| fail())
                };
                GlobalUnlock(data);
                result
            }
        }
    };
    if parsed.is_ok() && unsafe { GetClipboardSequenceNumber() } == sequence {
        unsafe {
            EmptyClipboard();
        }
    }
    unsafe {
        CloseClipboard();
    }
    parsed
}

unsafe fn allocate_bytes(bytes: &[u8]) -> Result<*mut c_void> {
    let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len());
    ensure!(!handle.is_null(), "clipboard allocation failed");
    let pointer = GlobalLock(handle);
    if pointer.is_null() {
        GlobalFree(handle);
        anyhow::bail!("clipboard allocation failed");
    }
    ptr::copy_nonoverlapping(bytes.as_ptr(), pointer.cast::<u8>(), bytes.len());
    GlobalUnlock(handle);
    Ok(handle)
}

unsafe fn allocate_u32(value: u32) -> Result<*mut c_void> {
    allocate_bytes(std::slice::from_raw_parts((&value as *const u32).cast(), 4))
}

unsafe fn free_handles(handles: &mut [*mut c_void]) {
    for handle in handles {
        if !handle.is_null() {
            free_handle(*handle);
        }
    }
}

unsafe fn free_handle(handle: *mut c_void) {
    if !handle.is_null() {
        let size = global_size(handle);
        let memory = GlobalLock(handle);
        if !memory.is_null() {
            ptr::write_bytes(memory.cast::<u8>(), 0, size);
            GlobalUnlock(handle);
        }
        GlobalFree(handle);
    }
}

unsafe fn global_size(handle: *mut c_void) -> usize {
    windows_sys::Win32::System::Memory::GlobalSize(handle)
}

unsafe fn format(name: &str) -> u32 {
    let wide = name
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    RegisterClipboardFormatW(wide.as_ptr())
}

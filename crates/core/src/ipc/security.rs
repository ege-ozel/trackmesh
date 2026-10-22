//! Per-user local pipe; never use the permissive default Windows pipe DACL.
use anyhow::{ensure, Result};
use std::{ffi::c_void, ptr};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows_sys::Win32::{
    Foundation::{CloseHandle, LocalFree},
    Security::{
        Authorization::{
            ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW,
        },
        GetTokenInformation, TokenUser, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

pub fn user_sid() -> Result<String> {
    // Buffers use pointer-sized alignment for TOKEN_USER. Windows allocates the SID string.
    unsafe {
        let mut token = ptr::null_mut();
        ensure!(
            OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) != 0,
            "Cannot read current Windows user token"
        );
        let result = (|| {
            let mut length = 0;
            GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut length);
            ensure!(
                length > 0 && length <= 64 * 1024,
                "Invalid Windows user token size"
            );
            let mut buffer = vec![0usize; (length as usize).div_ceil(size_of::<usize>())];
            ensure!(
                GetTokenInformation(
                    token,
                    TokenUser,
                    buffer.as_mut_ptr().cast(),
                    length,
                    &mut length
                ) != 0,
                "Cannot read Windows user SID"
            );
            let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
            let mut string = ptr::null_mut();
            ensure!(
                ConvertSidToStringSidW(user.User.Sid, &mut string) != 0,
                "Cannot format Windows user SID"
            );
            let mut count = 0;
            while *string.add(count) != 0 {
                count += 1;
            }
            let sid = String::from_utf16_lossy(std::slice::from_raw_parts(string, count));
            LocalFree(string.cast());
            Ok(sid)
        })();
        CloseHandle(token);
        result
    }
}

pub fn plugin_pipe_name(suffix: &str) -> Result<String> {
    ensure!(
        suffix.len() <= 32
            && suffix
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'-'),
        "IPC suffix must contain at most 32 ASCII letters, digits or hyphens"
    );
    let sid = user_sid()?;
    Ok(format!(r"\\.\pipe\TrackMesh-v1-{sid}-{suffix}"))
}

pub fn create_pipe(name: &str, first: bool) -> Result<NamedPipeServer> {
    let sid = user_sid()?;
    let descriptor = format!("D:P(A;;GA;;;{sid})")
        .encode_utf16()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // Windows copies this descriptor during CreateNamedPipe; free it afterwards.
    unsafe {
        let mut security: *mut c_void = ptr::null_mut();
        ensure!(
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                descriptor.as_ptr(),
                1,
                &mut security,
                ptr::null_mut()
            ) != 0,
            "Cannot create private IPC security descriptor"
        );
        let mut attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: security,
            bInheritHandle: 0,
        };
        let result = ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .max_instances(17)
            .in_buffer_size(protocol::MAX_IPC_BYTES as u32)
            .out_buffer_size(protocol::MAX_IPC_BYTES as u32)
            .create_with_security_attributes_raw(
                name,
                (&mut attributes as *mut SECURITY_ATTRIBUTES).cast(),
            );
        LocalFree(security);
        Ok(result?)
    }
}

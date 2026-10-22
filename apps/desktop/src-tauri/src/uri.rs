//! Per-user `trackmesh://` association used by the native invite handler.
use anyhow::{ensure, Context, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use std::ptr;
use supabase_client::validate_supabase_publishable_key;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
    REG_OPTION_NON_VOLATILE, REG_SZ,
};
use zeroize::Zeroizing;

pub fn register_protocol() -> Result<()> {
    let exe = std::env::current_exe().context("could not locate TrackMesh executable")?;
    let command = format!("\"{}\" \"%1\"", exe.display());
    set_default(r"Software\Classes\trackmesh", "URL:TrackMesh Protocol")?;
    set_value(r"Software\Classes\trackmesh", "URL Protocol", "")?;
    set_default(r"Software\Classes\trackmesh\shell\open\command", &command)
}

/// Prefix check only: a malformed link must still reach the running app as a visible error,
/// never fall through to startup argument parsing.
pub fn is_invite_link(value: &str) -> bool {
    value
        .get(.."trackmesh://".len())
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("trackmesh://"))
}

pub struct InviteService {
    pub ref_name: String,
    pub publishable_key: String,
}

pub struct ParsedInviteLink {
    pub service: Option<InviteService>,
    pub id: String,
    pub key: Zeroizing<[u8; 32]>,
}

pub fn parse_invite_link(value: &str) -> Result<ParsedInviteLink> {
    // All accepted URI components are ASCII. This bounds both clipboard allocation and parsing.
    ensure!(
        value.len() <= 512 && value.is_ascii(),
        "invalid invitation link"
    );
    let body = value
        .strip_prefix("trackmesh://invite/")
        .context("invalid invitation link")?;
    let (route, fragment) = body.split_once('#').context("invalid invitation link")?;
    ensure!(!fragment.contains('#'), "invalid invitation link");
    let (service, id) = if let Some(v2) = route.strip_prefix("v2/") {
        let mut fields = v2.split('/');
        let ref_name = fields.next().context("invalid invitation link")?;
        let publishable_key = fields.next().context("invalid invitation link")?;
        let id = fields.next().context("invalid invitation link")?;
        ensure!(fields.next().is_none(), "invalid invitation link");
        ensure!(
            (1..=63).contains(&ref_name.len())
                && ref_name
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit()),
            "invalid invitation link"
        );
        validate_supabase_publishable_key(publishable_key).context("invalid invitation link")?;
        (
            Some(InviteService {
                ref_name: ref_name.to_owned(),
                publishable_key: publishable_key.to_owned(),
            }),
            id,
        )
    } else {
        ensure!(!route.contains('/'), "invalid invitation link");
        (None, route)
    };
    let key = decode_component(id)?;
    let decoded_key = decode_component(fragment)?;
    Ok(ParsedInviteLink {
        service,
        id: URL_SAFE_NO_PAD.encode(key),
        key: decoded_key,
    })
}

fn decode_component(value: &str) -> Result<Zeroizing<[u8; 32]>> {
    ensure!(value.len() == 43, "invalid invitation link");
    let decoded = Zeroizing::new(
        URL_SAFE_NO_PAD
            .decode(value)
            .context("invalid invitation link")?,
    );
    ensure!(decoded.len() == 32, "invalid invitation link");
    let canonical = Zeroizing::new(URL_SAFE_NO_PAD.encode(&*decoded));
    ensure!(canonical.as_str() == value, "invalid invitation link");
    let mut result = [0u8; 32];
    result.copy_from_slice(&decoded);
    Ok(Zeroizing::new(result))
}

fn set_default(path: &str, value: &str) -> Result<()> {
    set_registry_value(path, None, value)
}

fn set_value(path: &str, name: &str, value: &str) -> Result<()> {
    set_registry_value(path, Some(name), value)
}

fn set_registry_value(path: &str, name: Option<&str>, value: &str) -> Result<()> {
    let path = path
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let name = name.map(|value| {
        value
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>()
    });
    let mut key: HKEY = ptr::null_mut();
    let result = unsafe {
        RegCreateKeyExW(
            HKEY_CURRENT_USER,
            path.as_ptr(),
            0,
            ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            ptr::null(),
            &mut key,
            ptr::null_mut(),
        )
    };
    ensure!(result == 0, "could not register TrackMesh link handler");
    let mut data = value
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let data_bytes = u32::try_from(data.len() * 2)?;
    let result = unsafe {
        RegSetValueExW(
            key,
            name.as_ref().map_or(ptr::null(), |value| value.as_ptr()),
            0,
            REG_SZ,
            data.as_mut_ptr().cast::<u8>() as *const u8,
            data_bytes,
        )
    };
    unsafe {
        RegCloseKey(key);
    }
    ensure!(result == 0, "could not register TrackMesh link handler");
    Ok(())
}

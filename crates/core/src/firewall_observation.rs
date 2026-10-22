//! Read-only Windows Defender Firewall policy observation for the desktop.
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct FirewallObservation {
    pub state: String,
    pub profiles: Vec<FirewallProfileObservation>,
    pub tcp_rules: RuleCandidates,
    pub udp_rules: RuleCandidates,
    pub third_party_products: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct FirewallProfileObservation {
    pub name: String,
    pub enabled: Option<bool>,
    pub block_all_inbound: Option<bool>,
    pub default_inbound: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct RuleCandidates {
    pub allow: String,
    pub block: String,
}

impl Default for FirewallObservation {
    fn default() -> Self {
        Self {
            state: "UNKNOWN".into(),
            profiles: Vec::new(),
            tcp_rules: RuleCandidates::unknown(),
            udp_rules: RuleCandidates::unknown(),
            third_party_products: "UNKNOWN".into(),
        }
    }
}

impl RuleCandidates {
    fn unknown() -> Self {
        Self {
            allow: "UNKNOWN".into(),
            block: "UNKNOWN".into(),
        }
    }
}

/// Read the current profile and candidate rules. Call only from `spawn_blocking`.
#[cfg(not(windows))]
pub fn observe_firewall(_listen_port: u16) -> FirewallObservation {
    FirewallObservation::default()
}

#[cfg(windows)]
pub fn observe_firewall(listen_port: u16) -> FirewallObservation {
    use windows::Win32::{
        Foundation::{S_FALSE, S_OK},
        System::Com::{CoInitializeEx, CoUninitialize, COINIT_APARTMENTTHREADED},
    };

    // SAFETY: COM is initialized and used synchronously on this caller's thread. The desktop
    // invokes this function only inside spawn_blocking; all COM interfaces are dropped before
    // the guard calls CoUninitialize on the same thread.
    let initialized = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
    if initialized != S_OK && initialized != S_FALSE {
        return FirewallObservation::default();
    }
    struct ComGuard;
    impl Drop for ComGuard {
        fn drop(&mut self) {
            // SAFETY: this guard is created only after successful S_OK/S_FALSE initialization
            // and is dropped on the same blocking worker thread.
            unsafe { CoUninitialize() }
        }
    }
    let guard = ComGuard;
    let result = unsafe { inspect(listen_port) };
    drop(guard);
    result.unwrap_or_default()
}

#[cfg(windows)]
unsafe fn inspect(listen_port: u16) -> windows::core::Result<FirewallObservation> {
    use windows::{
        core::{Interface, GUID},
        Win32::{
            NetworkManagement::WindowsFirewall::{
                INetFwProducts, INetFwRule, NetFwPolicy2, NET_FW_ACTION_ALLOW, NET_FW_ACTION_BLOCK,
                NET_FW_IP_PROTOCOL_TCP, NET_FW_IP_PROTOCOL_UDP, NET_FW_PROFILE2_DOMAIN,
                NET_FW_PROFILE2_PRIVATE, NET_FW_PROFILE2_PUBLIC, NET_FW_RULE_DIR_IN,
            },
            System::{
                Com::{CoCreateInstance, IDispatch, CLSCTX_INPROC_SERVER},
                Ole::IEnumVARIANT,
                Variant::VARIANT,
            },
        },
    };

    // SAFETY: `CoCreateInstance` activates the documented in-process firewall policy COM object;
    // the caller initialized COM on this same thread and requests only its read interfaces.
    let policy: windows::Win32::NetworkManagement::WindowsFirewall::INetFwPolicy2 =
        unsafe { CoCreateInstance(&NetFwPolicy2, None, CLSCTX_INPROC_SERVER)? };
    // SAFETY: `policy` is a valid interface returned by CoCreateInstance on this initialized thread.
    let bits = unsafe { policy.CurrentProfileTypes()? };
    let profile_specs = [
        (NET_FW_PROFILE2_DOMAIN.0, "DOMAIN", NET_FW_PROFILE2_DOMAIN),
        (
            NET_FW_PROFILE2_PRIVATE.0,
            "PRIVATE",
            NET_FW_PROFILE2_PRIVATE,
        ),
        (NET_FW_PROFILE2_PUBLIC.0, "PUBLIC", NET_FW_PROFILE2_PUBLIC),
    ];
    let mut output = FirewallObservation {
        state: "OBSERVED".into(),
        ..Default::default()
    };
    let products_class = GUID::from_u128(0xCC19079B82724D73BB70CDB533527B61);
    // SAFETY: the Firewall API coclass is used only to read its registered product count.
    // SAFETY: COM is initialized on this thread; this API call only acquires the product collection.
    output.third_party_products = match unsafe {
        CoCreateInstance::<_, INetFwProducts>(&products_class, None, CLSCTX_INPROC_SERVER)
    }
    // SAFETY: the returned interface is owned on this thread and Count is read-only.
    .and_then(|products| unsafe { products.Count() })
    {
        Ok(count) if count > 0 => "PRESENT".into(),
        Ok(0) => "ABSENT".into(),
        _ => "UNKNOWN".into(),
    };
    let mut active_bits = 0;
    for (bit, name, profile) in profile_specs {
        if bits & bit == 0 {
            continue;
        }
        active_bits |= bit;
        // SAFETY: profile is one of the documented profile constants and policy is a live interface.
        let enabled = unsafe { policy.get_FirewallEnabled(profile).ok().map(|v| v.0 != 0) };
        // SAFETY: same policy/profile validity as the preceding getter; this getter is read-only.
        let block = unsafe {
            policy
                .get_BlockAllInboundTraffic(profile)
                .ok()
                .map(|v| v.0 != 0)
        };
        // SAFETY: same policy/profile validity as the preceding getter; this getter is read-only.
        let inbound = unsafe {
            policy.get_DefaultInboundAction(profile).ok().map(|v| {
                if v.0 == 1 {
                    "ALLOW"
                } else if v.0 == 0 {
                    "BLOCK"
                } else {
                    "UNKNOWN"
                }
                .to_owned()
            })
        };
        output.profiles.push(FirewallProfileObservation {
            name: name.into(),
            enabled,
            block_all_inbound: block,
            default_inbound: inbound,
        });
    }

    // A bounded enumeration prevents a malformed or unexpectedly huge policy from monopolizing
    // the worker. Any incomplete enumeration leaves candidate results UNKNOWN.
    // SAFETY: policy is a valid interface on this initialized COM thread; Rules is read-only.
    let rules = unsafe { policy.Rules()? };
    // SAFETY: rules is a valid returned interface and Count does not mutate it.
    let count = unsafe { rules.Count()? };
    if !(0..=4096).contains(&count) {
        return Ok(FirewallObservation::default());
    }
    let unknown = rules._NewEnum()?.cast::<IEnumVARIANT>()?;
    let mut allow = [false; 2];
    let mut block = [false; 2];
    let mut uncertain = [false; 2];
    let mut seen = 0;
    loop {
        let mut variant = [VARIANT::default()];
        let mut fetched = 0u32;
        // SAFETY: the one-element initialized VARIANT slice and fetched pointer satisfy the
        // IEnumVARIANT::Next contract; each returned VARIANT is dropped by windows-rs RAII.
        let hr = unsafe { unknown.Next(&mut variant, &mut fetched) };
        if hr.is_err() {
            return Ok(FirewallObservation::default());
        }
        if fetched == 0 {
            break;
        }
        seen += 1;
        if seen > count || seen > 4096 {
            return Ok(FirewallObservation::default());
        }
        let dispatch = match IDispatch::try_from(&variant[0]) {
            Ok(dispatch) => dispatch,
            Err(_) => return Ok(FirewallObservation::default()),
        };
        let rule = dispatch.cast::<INetFwRule>()?;
        // SAFETY: rule was obtained by querying an enumerated firewall COM rule interface.
        let enabled = unsafe { rule.Enabled()? }.0 != 0;
        // SAFETY: rule remains owned on this initialized COM thread; this is a read-only getter.
        let direction = unsafe { rule.Direction()? }.0;
        if !enabled || direction != NET_FW_RULE_DIR_IN.0 {
            continue;
        }
        // SAFETY: rule remains a valid interface; Profiles is a read-only getter.
        let profiles = unsafe { rule.Profiles()? };
        if profiles != -1 && profiles & active_bits == 0 {
            continue;
        }
        // SAFETY: rule remains a valid interface; Protocol is a read-only getter.
        let protocol = unsafe { rule.Protocol()? };
        let indexes: &[usize] = match protocol {
            x if x == NET_FW_IP_PROTOCOL_TCP.0 => &[0],
            x if x == NET_FW_IP_PROTOCOL_UDP.0 => &[1],
            256 => &[0, 1],
            _ => continue,
        };
        // SAFETY: rule remains a valid interface; each following getter reads a rule property.
        let ports = unsafe { rule.LocalPorts()? }.to_string();
        // SAFETY: rule remains a valid interface; ApplicationName is read-only.
        let app = unsafe { rule.ApplicationName()? }.to_string();
        // SAFETY: rule remains a valid interface; ServiceName is read-only.
        let service = unsafe { rule.ServiceName()? }.to_string();
        // SAFETY: rule remains a valid interface; LocalAddresses is read-only.
        let local_addresses = unsafe { rule.LocalAddresses()? }.to_string();
        // SAFETY: rule remains a valid interface; RemoteAddresses is read-only.
        let remote_addresses = unsafe { rule.RemoteAddresses()? }.to_string();
        // SAFETY: rule remains a valid interface; RemotePorts is read-only.
        let remote_ports = unsafe { rule.RemotePorts()? }.to_string();
        // SAFETY: rule remains a valid interface; InterfaceTypes is read-only.
        let interface_types = unsafe { rule.InterfaceTypes()? }.to_string();
        // SAFETY: rule remains a valid interface; Interfaces is read-only.
        let interfaces = unsafe { rule.Interfaces()? };
        let app_matches = if app.is_empty() || app == "*" {
            Some(true)
        } else {
            std::env::current_exe()
                .ok()
                .map(|path| path.to_string_lossy().eq_ignore_ascii_case(&app))
        };
        if app_matches == Some(false) || !service.is_empty() {
            continue;
        }
        let port_matches = port_scope_matches(&ports, listen_port);
        if port_matches == Some(false) {
            continue;
        }
        let scope_known = local_addresses.trim() == "*"
            && remote_addresses.trim() == "*"
            && remote_ports.trim() == "*"
            && (interface_types.eq_ignore_ascii_case("All") || interface_types == "*")
            && interfaces.is_empty();
        // SAFETY: rule remains a valid interface; Action is read-only.
        let action = unsafe { rule.Action()? }.0;
        for index in indexes {
            if app_matches.is_none() || !scope_known || port_matches.is_none() {
                uncertain[*index] = true;
            } else if port_matches == Some(true) {
                match action {
                    x if x == NET_FW_ACTION_ALLOW.0 => allow[*index] = true,
                    x if x == NET_FW_ACTION_BLOCK.0 => block[*index] = true,
                    _ => uncertain[*index] = true,
                }
            }
        }
    }
    if seen != count {
        return Ok(FirewallObservation::default());
    }
    if listen_port == 0 {
        // Port zero requests an ephemeral listener; its effective TCP/QUIC ports are unknown here.
        output.tcp_rules = RuleCandidates::unknown();
        output.udp_rules = RuleCandidates::unknown();
    } else {
        output.tcp_rules = candidates(allow[0], block[0], uncertain[0]);
        output.udp_rules = candidates(allow[1], block[1], uncertain[1]);
    }
    Ok(output)
}

#[cfg(windows)]
fn port_scope_matches(scope: &str, port: u16) -> Option<bool> {
    let scope = scope.trim();
    if scope == "*" {
        return Some(true);
    }
    if scope.is_empty() {
        return None;
    }
    let wanted = port.to_string();
    let mut parsed = false;
    for item in scope.split(',').map(str::trim) {
        if item == wanted {
            parsed = true;
            continue;
        }
        if let Some((first, last)) = item.split_once('-') {
            let first = first.parse::<u16>().ok()?;
            let last = last.parse::<u16>().ok()?;
            if first <= port && port <= last {
                return Some(true);
            }
            parsed = true;
            continue;
        }
        if item.parse::<u16>().is_err() {
            return None;
        }
        parsed = true;
    }
    Some(parsed && scope.split(',').any(|item| item.trim() == wanted))
}

#[cfg(windows)]
fn candidates(allow: bool, block: bool, uncertain: bool) -> RuleCandidates {
    let state = |found| {
        if found {
            "PRESENT"
        } else if uncertain {
            "UNKNOWN"
        } else {
            "ABSENT"
        }
    };
    RuleCandidates {
        allow: state(allow).into(),
        block: state(block).into(),
    }
}

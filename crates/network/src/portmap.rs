//! Explicitly consented, finite UPnP mappings owned by the peer runtime.
use crate::{Network, PortMappingStatus};
use igd_next::{
    aio::{tokio::search_gateway, Gateway},
    PortMappingProtocol, SearchOptions,
};
use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    time::Duration,
};
use tokio::{
    task::JoinHandle,
    time::{timeout, Instant},
};

#[derive(Clone, Copy)]
pub struct UpnpTarget {
    pub gateway: IpAddr,
    pub local: Ipv4Addr,
}

pub struct PortMappingRuntime {
    task: JoinHandle<()>,
    cancel: tokio::sync::watch::Sender<bool>,
}
impl PortMappingRuntime {
    pub fn start(network: Network, target: Option<UpnpTarget>, device_id: String) -> Self {
        let mut consent = network.mapping_consent.subscribe();
        let (cancel, mut cancelled) = tokio::sync::watch::channel(false);
        let task = tokio::spawn(async move {
            run(network, target, device_id, &mut consent, &mut cancelled).await
        });
        Self { task, cancel }
    }
    pub async fn stop(mut self, network: &Network) {
        self.stop_inner(network, true).await;
    }
    pub async fn stop_for_target_change(mut self, network: &Network) {
        self.stop_inner(network, false).await;
    }
    async fn stop_inner(&mut self, network: &Network, revoke_consent: bool) {
        if revoke_consent {
            network.set_mapping_consent(false);
        }
        self.cancel.send_replace(true);
        let deadline = Instant::now() + Duration::from_secs(15);
        let mut worker_stopped = timeout(Duration::from_secs(10), &mut self.task)
            .await
            .is_ok();
        if !worker_stopped {
            self.task.abort();
            tracing::warn!("cleanup could not be confirmed; finite lease will expire");
            let remaining = deadline.saturating_duration_since(Instant::now());
            worker_stopped = timeout(remaining, &mut self.task).await.is_ok();
        }
        if worker_stopped {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let _ = timeout(remaining, network.set_external_addresses(Vec::new())).await;
        }
    }
}

async fn run(
    network: Network,
    target: Option<UpnpTarget>,
    device_id: String,
    consent: &mut tokio::sync::watch::Receiver<bool>,
    cancel: &mut tokio::sync::watch::Receiver<bool>,
) {
    loop {
        if *cancel.borrow() {
            return;
        }
        if !*consent.borrow() {
            tokio::select! { _ = cancel.changed() => return, changed = consent.changed() => if changed.is_err() { return; } }
            continue;
        }
        let Some(target) = target else {
            set_status(
                &network,
                status(
                    "UNKNOWN",
                    "UNKNOWN",
                    "UNKNOWN",
                    Some(
                        "A single local IPv4 interface and default gateway could not be identified",
                    ),
                ),
            );
            tokio::select! { _ = cancel.changed() => return, changed = consent.changed() => if changed.is_err() { return; } }
            continue;
        };
        set_status(&network, status("DISCOVERING", "PENDING", "PENDING", None));
        let mut options = SearchOptions::default();
        options.bind_addr = SocketAddr::new(IpAddr::V4(target.local), 0);
        options.timeout = Some(Duration::from_secs(5));
        options.single_search_timeout = Some(Duration::from_secs(1));
        options.allowed_gateway_ips = vec![target.gateway];
        let gateway = tokio::select! {
            _ = cancel.changed() => return,
            _ = consent.changed() => continue,
            result = search_gateway(options) => match result {
                Ok(gateway) if gateway.addr.ip() == target.gateway => gateway,
                Ok(_) => {
                    set_status(&network, status("UNKNOWN", "UNKNOWN", "UNKNOWN", Some("UPnP responder did not match the observed default gateway")));
                    tokio::select! { _ = cancel.changed() => return, changed = consent.changed() => if changed.is_err() { return; } }
                    continue;
                }
                Err(_) => {
                    set_status(&network, status("FAILED", "FAILED", "FAILED", Some("UPnP gateway discovery failed")));
                    tokio::select! { _ = cancel.changed() => return, changed = consent.changed() => if changed.is_err() { return; } }
                    continue;
                }
            }
        };
        let router_external_ip = tokio::select! {
            _ = cancel.changed() => return,
            result = timeout(Duration::from_secs(5), gateway.get_external_ip()) => match result {
                Ok(Ok(std::net::IpAddr::V4(ip))) => Some(ip),
                _ => None,
            }
        };
        let mut external_ip_state = match router_external_ip {
            Some(ip) if is_public_ipv4(ip) => "PUBLIC",
            Some(ip) if is_private_or_cgnat_ipv4(ip) => "PRIVATE_OR_CGNAT",
            None => "UNKNOWN",
            Some(_) => "UNKNOWN",
        };
        let mut external_ip = router_external_ip.filter(|ip| is_public_ipv4(*ip));
        let description = format!(
            "TrackMesh {}",
            device_id.chars().take(8).collect::<String>()
        );
        let mut owned = Vec::new();
        let mut tcp_state = "FAILED";
        let mut udp_state = "FAILED";
        let deadline = Instant::now() + Duration::from_secs(30);
        let ports = loop {
            let ports = listen_ports(&network, target.local);
            if ports.0.is_some() && ports.1.is_some() {
                break ports;
            }
            if Instant::now() >= deadline {
                break ports;
            }
            tokio::select! { _ = cancel.changed() => return, _ = consent.changed() => break (None, None), _ = tokio::time::sleep(Duration::from_millis(250)) => {} }
            if !*consent.borrow() {
                break (None, None);
            }
        };
        if let Some(port) = ports.0 {
            let addr = SocketAddr::new(IpAddr::V4(target.local), port);
            match tokio::time::timeout(
                Duration::from_secs(5),
                mapping_is_available(
                    &gateway,
                    PortMappingProtocol::TCP,
                    port,
                    target.local,
                    &description,
                ),
            )
            .await
            .unwrap_or(false)
            {
                true if *consent.borrow() && !*cancel.borrow() => match timeout(
                    Duration::from_secs(5),
                    gateway.add_port(PortMappingProtocol::TCP, port, addr, 3600, &description),
                )
                .await
                {
                    Ok(Ok(())) => {
                        tcp_state = "MAPPED";
                        owned.push((PortMappingProtocol::TCP, port));
                    }
                    _ => tcp_state = "DENIED",
                },
                true => tcp_state = "NOT_ATTEMPTED",
                false => tcp_state = "DENIED",
            }
        }
        if *consent.borrow() {
            if let Some(port) = ports.1 {
                let addr = SocketAddr::new(IpAddr::V4(target.local), port);
                match tokio::time::timeout(
                    Duration::from_secs(5),
                    mapping_is_available(
                        &gateway,
                        PortMappingProtocol::UDP,
                        port,
                        target.local,
                        &description,
                    ),
                )
                .await
                .unwrap_or(false)
                {
                    true if *consent.borrow() && !*cancel.borrow() => match timeout(
                        Duration::from_secs(5),
                        gateway.add_port(PortMappingProtocol::UDP, port, addr, 3600, &description),
                    )
                    .await
                    {
                        Ok(Ok(())) => {
                            udp_state = "MAPPED";
                            owned.push((PortMappingProtocol::UDP, port));
                        }
                        _ => udp_state = "DENIED",
                    },
                    true => udp_state = "NOT_ATTEMPTED",
                    false => udp_state = "DENIED",
                }
            }
        }
        let state = if tcp_state == "MAPPED" || udp_state == "MAPPED" {
            "MAPPED"
        } else {
            "FAILED"
        };
        let detail = if external_ip_state == "PRIVATE_OR_CGNAT" {
            "Router-reported external IPv4 is private or CGNAT; inbound reachability remains UNVERIFIED"
        } else {
            "Mappings are time-limited; inbound reachability is UNVERIFIED"
        };
        let mut mapping_status = status(state, tcp_state, udp_state, Some(detail));
        mapping_status.external_ip_state = external_ip_state.into();
        set_status(&network, mapping_status);
        let addresses = if *consent.borrow() && !*cancel.borrow() {
            mapped_external_addresses(external_ip, tcp_state, udp_state, ports)
        } else {
            Vec::new()
        };
        let _ = network.set_external_addresses(addresses).await;
        let mut cleanup_failed = false;
        if !owned.is_empty() {
            let mut renew_at = Instant::now() + Duration::from_secs(1800);
            loop {
                tokio::select! {
                    _ = cancel.changed() => break,
                    _ = consent.changed() => break,
                    _ = tokio::time::sleep_until(renew_at) => {
                        match timeout(Duration::from_secs(5), gateway.get_external_ip()).await {
                            Ok(Ok(std::net::IpAddr::V4(ip))) if is_public_ipv4(ip) => {
                                external_ip = Some(ip);
                                external_ip_state = "PUBLIC";
                            }
                            Ok(Ok(std::net::IpAddr::V4(ip))) if is_private_or_cgnat_ipv4(ip) => {
                                external_ip = None;
                                external_ip_state = "PRIVATE_OR_CGNAT";
                            }
                            _ => {
                                external_ip = None;
                                external_ip_state = "UNKNOWN";
                            }
                        }
                        for (protocol, port) in &owned {
                            let addr = SocketAddr::new(IpAddr::V4(target.local), *port);
                            let check = tokio::time::timeout(Duration::from_secs(5), mapping_state(&gateway, *protocol, *port, target.local, &description)).await.unwrap_or(MappingPresence::Unknown);
                            if *consent.borrow() && !*cancel.borrow() && matches!(check, MappingPresence::Vacant | MappingPresence::Owned) {
                                let state = timeout(Duration::from_secs(5), gateway.add_port(*protocol, *port, addr, 3600, &description)).await;
                                let protocol_state = if matches!(state, Ok(Ok(()))) { "MAPPED" } else { "EXPIRED" };
                                if *protocol == PortMappingProtocol::TCP { tcp_state = protocol_state; } else { udp_state = protocol_state; }
                            } else {
                                if *protocol == PortMappingProtocol::TCP { tcp_state = "EXPIRED"; } else { udp_state = "EXPIRED"; }
                            }
                        }
                        let state = if tcp_state == "MAPPED" || udp_state == "MAPPED" { "MAPPED" } else if tcp_state == "EXPIRED" || udp_state == "EXPIRED" { "EXPIRED" } else { "FAILED" };
                        let reason = if tcp_state == "EXPIRED" && udp_state == "EXPIRED" { "TCP and UDP mapping lease renewal failed; mappings are EXPIRED" } else if tcp_state == "EXPIRED" { "TCP mapping lease renewal failed; TCP is EXPIRED" } else if udp_state == "EXPIRED" { "UDP mapping lease renewal failed; UDP is EXPIRED" } else { "UPnP mapping lease renewal status" };
                        let mut renewed = status(state, tcp_state, udp_state, Some(reason));
                        renewed.external_ip_state = external_ip_state.into();
                        set_status(&network, renewed);
                        let addresses = if *consent.borrow() && !*cancel.borrow() {
                            mapped_external_addresses(external_ip, tcp_state, udp_state, ports)
                        } else {
                            Vec::new()
                        };
                        let _ = network.set_external_addresses(addresses).await;
                        renew_at = Instant::now() + Duration::from_secs(1800);
                    }
                }
            }
            let _ = network.set_external_addresses(Vec::new()).await;
            for (protocol, port) in owned {
                let check = tokio::time::timeout(
                    Duration::from_secs(5),
                    mapping_state(&gateway, protocol, port, target.local, &description),
                )
                .await
                .unwrap_or(MappingPresence::Unknown);
                if matches!(check, MappingPresence::Owned) {
                    if !matches!(
                        timeout(Duration::from_secs(5), gateway.remove_port(protocol, port)).await,
                        Ok(Ok(()))
                    ) {
                        cleanup_failed = true;
                    }
                } else if matches!(check, MappingPresence::Unknown) {
                    cleanup_failed = true;
                }
            }
        }
        if !*consent.borrow() {
            if cleanup_failed {
                tracing::warn!(
                    "UPnP mapping cleanup could not be confirmed; the finite lease will expire"
                );
                set_status(&network, {
                    let mut failed = status(
                        "FAILED",
                        "FAILED",
                        "FAILED",
                        Some(
                            "Mapping cleanup could not be confirmed; the finite lease will expire",
                        ),
                    );
                    failed.consent_enabled = false;
                    failed
                });
            } else {
                set_status(&network, PortMappingStatus::default());
            }
            tokio::select! { _ = cancel.changed() => return, changed = consent.changed() => if changed.is_err() { return; } }
        } else {
            if *cancel.borrow() {
                return;
            }
            continue;
        }
        if *cancel.borrow() {
            return;
        }
    }
}

fn listen_ports(network: &Network, local: Ipv4Addr) -> (Option<u16>, Option<u16>) {
    let mut tcp = Vec::new();
    let mut udp = Vec::new();
    for value in network.status().addresses {
        let pieces = value.split('/').collect::<Vec<_>>();
        let Some(index) = pieces.iter().position(|part| *part == "ip4") else {
            continue;
        };
        let Some(host) = pieces
            .get(index + 1)
            .and_then(|value| value.parse::<Ipv4Addr>().ok())
        else {
            continue;
        };
        if !host.is_unspecified() && host != local {
            continue;
        }
        if let Some(i) = pieces.iter().position(|p| *p == "tcp") {
            if let Some(port) = pieces
                .get(i + 1)
                .and_then(|p| p.parse::<u16>().ok())
                .filter(|p| *p != 0)
            {
                tcp.push(port);
            }
        }
        if let Some(i) = pieces.iter().position(|p| *p == "udp") {
            if let Some(port) = pieces
                .get(i + 1)
                .and_then(|p| p.parse::<u16>().ok())
                .filter(|p| *p != 0)
            {
                udp.push(port);
            }
        }
    }
    tcp.sort_unstable();
    tcp.dedup();
    udp.sort_unstable();
    udp.dedup();
    (
        if tcp.len() == 1 {
            tcp.first().copied()
        } else {
            None
        },
        if udp.len() == 1 {
            udp.first().copied()
        } else {
            None
        },
    )
}
#[derive(Clone, Copy)]
enum MappingPresence {
    Vacant,
    Owned,
    Occupied,
    Unknown,
}

async fn mapping_is_available(
    gateway: &Gateway<igd_next::aio::tokio::Tokio>,
    protocol: PortMappingProtocol,
    port: u16,
    local: Ipv4Addr,
    description: &str,
) -> bool {
    matches!(
        mapping_state(gateway, protocol, port, local, description).await,
        MappingPresence::Vacant | MappingPresence::Owned
    )
}

async fn mapping_state(
    gateway: &Gateway<igd_next::aio::tokio::Tokio>,
    protocol: PortMappingProtocol,
    port: u16,
    local: Ipv4Addr,
    description: &str,
) -> MappingPresence {
    for index in 0..256 {
        match gateway.get_generic_port_mapping_entry(index).await {
            Ok(entry) if entry.external_port == port && entry.protocol == protocol => {
                return if entry.internal_port == port
                    && entry.internal_client == local.to_string()
                    && entry.port_mapping_description == description
                {
                    MappingPresence::Owned
                } else {
                    MappingPresence::Occupied
                };
            }
            Ok(_) => {}
            Err(igd_next::GetGenericPortMappingEntryError::SpecifiedArrayIndexInvalid) => {
                return MappingPresence::Vacant
            }
            Err(_) => return MappingPresence::Unknown,
        }
    }
    MappingPresence::Unknown
}
fn status(state: &str, tcp: &str, udp: &str, detail: Option<&str>) -> PortMappingStatus {
    PortMappingStatus {
        consent_enabled: true,
        state: state.into(),
        tcp: tcp.into(),
        udp: udp.into(),
        detail: detail.map(str::to_owned),
        external_ip_state: "UNKNOWN".into(),
    }
}
fn is_public_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !ip.is_private()
        && !ip.is_loopback()
        && !ip.is_link_local()
        && !ip.is_documentation()
        && !ip.is_broadcast()
        && a != 0
        && a < 224
        && !(a == 100 && (64..=127).contains(&b))
        && !(a == 198 && (b == 18 || b == 19))
        && !(a == 192 && ((b == 0 && c == 0) || (b == 88 && c == 99)))
}
fn is_private_or_cgnat_ipv4(ip: Ipv4Addr) -> bool {
    ip.is_private()
        || ip.is_link_local()
        || (ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]))
}
fn mapped_external_addresses(
    external_ip: Option<Ipv4Addr>,
    tcp_state: &str,
    udp_state: &str,
    ports: (Option<u16>, Option<u16>),
) -> Vec<libp2p::Multiaddr> {
    let Some(ip) = external_ip.filter(|ip| is_public_ipv4(*ip)) else {
        return Vec::new();
    };
    let mut addresses = Vec::new();
    if tcp_state == "MAPPED" {
        if let Some(port) = ports.0 {
            if let Ok(address) = format!("/ip4/{ip}/tcp/{port}").parse() {
                addresses.push(address);
            }
        }
    }
    if udp_state == "MAPPED" {
        if let Some(port) = ports.1 {
            if let Ok(address) = format!("/ip4/{ip}/udp/{port}/quic-v1").parse() {
                addresses.push(address);
            }
        }
    }
    addresses
}
fn set_status(network: &Network, mapping: PortMappingStatus) {
    if let Ok(mut state) = network.status.write() {
        state.port_mapping = mapping;
    }
}

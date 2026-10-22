use anyhow::{ensure, Result};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use uuid::Uuid;

pub const ENDPOINT_DOMAIN: &str = "trackmesh-endpoint-v1";
pub const MAX_ENDPOINT_ADDRESSES: usize = 16;
pub const MAX_ENDPOINT_RECORD_BYTES: usize = 16 * 1024;
pub const MAX_ENDPOINT_REQUEST_BYTES: usize = MAX_ENDPOINT_RECORD_BYTES + 256;
pub const PREFERRED_ENDPOINT_TTL: Duration = Duration::minutes(10);
pub const MAX_ENDPOINT_TTL: Duration = Duration::minutes(15);
const MAX_FUTURE_SKEW: Duration = Duration::minutes(2);

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct EndpointRecord {
    pub device_id: Uuid,
    pub peer_id: String,
    pub addresses: Vec<String>,
    pub issued_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub seq: u64,
    pub signature: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointSequence {
    Newer,
    Duplicate,
}

impl EndpointRecord {
    pub fn signing_bytes(&self) -> Result<Vec<u8>> {
        Ok(serde_json::to_vec(&(
            ENDPOINT_DOMAIN,
            self.device_id,
            &self.peer_id,
            &self.addresses,
            self.issued_at,
            self.expires_at,
            self.seq,
        ))?)
    }

    pub fn validate(&self, now: DateTime<Utc>) -> Result<()> {
        let encoded = serde_json::to_vec(self)?;
        ensure!(
            encoded.len() <= MAX_ENDPOINT_RECORD_BYTES,
            "endpoint record too large"
        );
        ensure!(
            !self.peer_id.is_empty() && self.peer_id.len() <= 128,
            "invalid peer id"
        );
        ensure!(
            self.addresses.len() <= MAX_ENDPOINT_ADDRESSES,
            "too many endpoint addresses"
        );
        ensure!(
            self.issued_at <= now + MAX_FUTURE_SKEW,
            "endpoint issued too far in the future"
        );
        let ttl = self.expires_at - self.issued_at;
        ensure!(
            ttl > Duration::zero() && ttl <= MAX_ENDPOINT_TTL,
            "invalid endpoint lifetime"
        );
        ensure!(now < self.expires_at, "endpoint record expired");
        let mut previous: Option<&str> = None;
        for address in &self.addresses {
            validate_endpoint_address(address)?;
            ensure!(
                previous.is_none_or(|prev| prev < address.as_str()),
                "endpoint addresses must be sorted and unique"
            );
            previous = Some(address);
        }
        Ok(())
    }

    pub fn compare_sequence(&self, previous: &Self) -> Result<EndpointSequence> {
        ensure!(
            self.device_id == previous.device_id,
            "endpoint sequence device mismatch"
        );
        ensure!(self.seq >= previous.seq, "stale endpoint sequence");
        if self.seq == previous.seq {
            ensure!(self == previous, "endpoint sequence equivocation");
            return Ok(EndpointSequence::Duplicate);
        }
        Ok(EndpointSequence::Newer)
    }
}

pub fn validate_endpoint_address(address: &str) -> Result<()> {
    ensure!(
        address.len() <= 1024 && address.starts_with('/'),
        "invalid endpoint address"
    );
    let parts: Vec<&str> = address.split('/').skip(1).collect();
    ensure!(
        parts.len() == 4 || parts.len() == 5,
        "unsupported endpoint address shape"
    );
    let (family, ip, transport, port, quic) = if parts.len() == 4 {
        (parts[0], parts[1], parts[2], parts[3], false)
    } else {
        ensure!(parts[4] == "quic-v1", "unsupported endpoint transport");
        (parts[0], parts[1], parts[2], parts[3], true)
    };
    let parsed: IpAddr = ip.parse()?;
    ensure!(parsed.to_string() == ip, "endpoint IP is not canonical");
    ensure!(
        port.parse::<u16>()? != 0 && port.parse::<u16>()?.to_string() == port,
        "invalid endpoint port"
    );
    ensure!(
        matches!(transport, "tcp" | "udp"),
        "unsupported endpoint transport"
    );
    ensure!(
        (transport == "udp") == quic,
        "UDP endpoints must use QUIC v1"
    );
    match (family, parsed) {
        ("ip4", IpAddr::V4(ip)) => {
            ensure!(ipv4_endpoint_candidate(ip), "non-routable endpoint address")
        }
        ("ip6", IpAddr::V6(ip)) => {
            ensure!(ipv6_endpoint_candidate(ip), "non-routable endpoint address")
        }
        _ => anyhow::bail!("endpoint address family mismatch"),
    }
    Ok(())
}

fn ipv4_endpoint_candidate(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    ip.is_private()
        || ((1..=223).contains(&a)
            && !ip.is_loopback()
            && !ip.is_link_local()
            && !ip.is_broadcast()
            && !(a == 192 && b == 0 && c == 0)
            && !(a == 192 && b == 0 && c == 2)
            && !(a == 192 && b == 88 && c == 99)
            && !(a == 198 && (b == 18 || b == 19))
            && !(a == 198 && b == 51 && c == 100)
            && !(a == 203 && b == 0 && c == 113)
            && !(a == 100 && (64..=127).contains(&b)))
}

fn ipv6_endpoint_candidate(ip: Ipv6Addr) -> bool {
    if ip.is_unique_local() {
        return true;
    }
    let s = ip.segments();
    let global = (s[0] & 0xe000) == 0x2000;
    let protocol_special = (s[0] == 0x2001 && ((s[1] & 0xfe00) == 0 || s[1] == 0x0db8))
        || s[0] == 0x2002
        || (s[0] == 0x2620 && s[1] == 0x004f && s[2] == 0x8000)
        || (s[0] == 0x3fff && (s[1] & 0xfff0) == 0)
        || s[0] == 0x5f00;
    global
        && !protocol_special
        && !ip.is_unspecified()
        && !ip.is_loopback()
        && !ip.is_unicast_link_local()
        && !ip.is_multicast()
        && ip.to_ipv4_mapped().is_none()
}

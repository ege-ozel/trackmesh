//! Public endpoint construction for adapters; no network lifecycle changes.
use anyhow::{ensure, Context, Result};
use std::net::Ipv4Addr;

pub fn public_ipv4_addresses(ip: &str, port: u16) -> Result<Vec<String>> {
    ensure!(
        ip.len() <= 15 && port > 0,
        "Enter a public IPv4 address and a port from 1 to 65535"
    );
    let ip: Ipv4Addr = ip.parse().context("Enter a valid public IPv4 address")?;
    let [a, b, _, _] = ip.octets();
    ensure!(
        !ip.is_private()
            && !ip.is_loopback()
            && !ip.is_link_local()
            && !ip.is_documentation()
            && !ip.is_broadcast()
            && a != 0
            && a < 224
            && !(a == 100 && (64..=127).contains(&b))
            && !(a == 198 && (b == 18 || b == 19)),
        "This IPv4 address is not public; check the router WAN address and CGNAT with your ISP"
    );
    Ok(vec![
        format!("/ip4/{ip}/tcp/{port}"),
        format!("/ip4/{ip}/udp/{port}/quic-v1"),
    ])
}

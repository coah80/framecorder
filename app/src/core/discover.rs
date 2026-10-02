//! Finding Frames on the local network: over mDNS, and by asking every
//! address nearby directly, for networks that drop multicast (lots of
//! routers, guest Wi-Fi, the Windows firewall).

use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use mdns_sd::{ServiceDaemon, ServiceEvent};
use serde::Serialize;

use super::api::Client;
use super::pairlink::join_addr;
use super::tls::normalize_fingerprint;

pub const SERVICE: &str = "_framecorder._tcp.local.";
/// Where framecorder-sync listens, unless something else had that port.
pub const PORT: u16 = 38619;
/// How long a closed or empty address gets to answer a knock.
const KNOCK: Duration = Duration::from_millis(700);
/// Knocks in flight at once. A /24 takes about three rounds.
const PARALLEL: usize = 96;
const HELLO: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Found {
    pub name: String,
    pub fingerprint: String,
    /// Best guess of `addrs`.
    pub addr: String,
    /// Everything it advertised. A machine with VPNs or containers has
    /// addresses we can't reach, so callers try them all.
    pub addrs: Vec<String>,
}

fn found_from(info: &mdns_sd::ResolvedService) -> Option<Found> {
    let fingerprint = normalize_fingerprint(info.get_property_val_str("fp")?)?;
    let name = info.get_property_val_str("name").unwrap_or("Steam Frame").to_string();
    // IPv4 first, it's what the Frame advertises and what always routes
    let mut ips: Vec<IpAddr> = info.get_addresses().iter().map(|a| a.to_ip_addr()).collect();
    ips.sort_by_key(|ip| (!ip.is_ipv4(), ip.is_loopback(), *ip));
    let addrs: Vec<String> = ips.iter().map(|ip| join_addr(&ip.to_string(), info.get_port())).collect();
    Some(Found { name, fingerprint, addr: addrs.first()?.clone(), addrs })
}

/// Browses for `window`, calling `on_found` for every Frame that shows up.
/// Stops early once `on_found` returns false.
pub async fn browse(window: Duration, mut on_found: impl FnMut(Found) -> bool) -> Result<(), String> {
    let mdns = ServiceDaemon::new().map_err(|e| format!("mDNS isn't available: {e}"))?;
    let rx = mdns.browse(SERVICE).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + window;
    let mut seen: HashMap<String, Found> = HashMap::new();
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            break;
        }
        match tokio::time::timeout(left, rx.recv_async()).await {
            Ok(Ok(ServiceEvent::ServiceResolved(info))) => {
                if let Some(found) = found_from(&info) {
                    if seen.get(&found.fingerprint) != Some(&found) {
                        seen.insert(found.fingerprint.clone(), found.clone());
                        if !on_found(found) {
                            break;
                        }
                    }
                }
            }
            Ok(Ok(_)) => {}
            Ok(Err(_)) | Err(_) => break,
        }
    }
    let _ = mdns.stop_browse(SERVICE);
    let _ = mdns.shutdown();
    Ok(())
}

/// The addresses a sweep knocks on: every host of each private IPv4
/// network this device is on, at most a /24 of each, never itself.
pub fn sweep_targets() -> Vec<Ipv4Addr> {
    let ifaces = if_addrs::get_if_addrs().unwrap_or_default();
    targets_of(ifaces.iter().filter_map(|i| match &i.addr {
        if_addrs::IfAddr::V4(v4) => Some((v4.ip, v4.netmask)),
        _ => None,
    }))
}

fn targets_of(nets: impl Iterator<Item = (Ipv4Addr, Ipv4Addr)>) -> Vec<Ipv4Addr> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for (ip, netmask) in nets {
        if ip.is_loopback() || !ip.is_private() {
            continue;
        }
        // a /16 home network would be 65k knocks; the Frame is nearly always in our /24
        let mask = u32::from(netmask).max(0xffff_ff00);
        let (me, net) = (u32::from(ip), u32::from(ip) & mask);
        for host in 1..!mask {
            let other = net | host;
            if other != me && seen.insert(other) {
                out.push(Ipv4Addr::from(other));
            }
        }
    }
    out
}

/// Asks whatever answers at `addr` who it is. Only a framecorder-sync whose
/// certificate matches the fingerprint it claims counts.
pub async fn identify(addr: &str) -> Option<Found> {
    let client = Client::capture(addr).ok()?;
    let hello = tokio::time::timeout(HELLO, client.hello()).await.ok()?.ok()?;
    let seen = client.seen_fingerprint()?;
    if normalize_fingerprint(&hello.fingerprint)? != seen {
        return None;
    }
    Some(Found { name: hello.name, fingerprint: seen, addr: addr.to_string(), addrs: vec![addr.to_string()] })
}

/// Knocks on port 38619 of every nearby address for up to `window`, and
/// says hello to whatever opens. Works where multicast doesn't.
pub async fn sweep(window: Duration, mut on_found: impl FnMut(Found) -> bool) {
    let targets = sweep_targets();
    if targets.is_empty() {
        return;
    }
    let open = futures_util::stream::iter(targets)
        .map(|ip| async move {
            let addr = SocketAddr::from((ip, PORT));
            matches!(tokio::time::timeout(KNOCK, tokio::net::TcpStream::connect(addr)).await, Ok(Ok(_))).then_some(addr)
        })
        .buffer_unordered(PARALLEL)
        .filter_map(|a| async move { a })
        .map(|addr| async move { identify(&addr.to_string()).await })
        .buffer_unordered(8)
        .filter_map(|f| async move { f });
    let run = async {
        futures_util::pin_mut!(open);
        while let Some(found) = open.next().await {
            if !on_found(found) {
                break;
            }
        }
    };
    let _ = tokio::time::timeout(window, run).await;
}

/// mDNS (unless `mdns` is off) and the sweep at once. Each Frame is reported
/// when it first shows up, and again if it turns up at more addresses.
pub async fn look(window: Duration, mdns: bool, mut on_found: impl FnMut(Found) -> bool) {
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Found>();
    let by_name = tx.clone();
    let producers = async move {
        let browsing = async move {
            if mdns {
                if let Err(e) = browse(window, |f| by_name.send(f).is_ok()).await {
                    log::info!("{e}, only sweeping");
                }
            }
        };
        let sweeping = sweep(window, move |f| tx.send(f).is_ok());
        tokio::join!(browsing, sweeping);
    };
    tokio::spawn(producers);

    let mut seen: HashMap<String, Found> = HashMap::new();
    while let Some(found) = rx.recv().await {
        let merged = match seen.get(&found.fingerprint) {
            Some(old) => {
                let mut merged = old.clone();
                for a in &found.addrs {
                    if !merged.addrs.contains(a) {
                        merged.addrs.push(a.clone());
                    }
                }
                if merged == *old {
                    continue;
                }
                merged
            }
            None => found,
        };
        seen.insert(merged.fingerprint.clone(), merged.clone());
        if !on_found(merged) {
            break;
        }
    }
}

/// Looks for one particular Frame, e.g. after its address changed.
pub async fn find(fingerprint: &str, window: Duration, mdns: bool) -> Option<Found> {
    let mut hit = None;
    look(window, mdns, |f| {
        if f.fingerprint == fingerprint {
            hit = Some(f);
            false
        } else {
            true
        }
    })
    .await;
    hit
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v4(s: &str) -> Ipv4Addr {
        s.parse().unwrap()
    }

    #[test]
    fn sweeps_the_local_slash_24_only() {
        let t = targets_of([(v4("192.168.1.23"), v4("255.255.255.0"))].into_iter());
        assert_eq!(t.len(), 253);
        assert!(!t.contains(&v4("192.168.1.23")));
        assert!(!t.contains(&v4("192.168.1.0")) && !t.contains(&v4("192.168.1.255")));
        assert!(t.contains(&v4("192.168.1.1")) && t.contains(&v4("192.168.1.254")));
        // a /16 gets narrowed to our /24
        let wide = targets_of([(v4("10.0.5.9"), v4("255.255.0.0"))].into_iter());
        assert_eq!(wide.len(), 253);
        assert!(wide.iter().all(|ip| ip.octets()[2] == 5));
        // a /25 stays a /25
        assert_eq!(targets_of([(v4("192.168.1.130"), v4("255.255.255.128"))].into_iter()).len(), 125);
    }

    #[test]
    fn never_sweeps_public_or_loopback_networks() {
        let t = targets_of([(v4("127.0.0.1"), v4("255.0.0.0")), (v4("8.8.8.8"), v4("255.255.255.0"))].into_iter());
        assert!(t.is_empty());
    }

    #[test]
    fn two_interfaces_on_one_network_knock_once() {
        let t = targets_of([(v4("192.168.1.23"), v4("255.255.255.0")), (v4("192.168.1.24"), v4("255.255.255.0"))].into_iter());
        assert_eq!(t.len(), 254);
    }
}

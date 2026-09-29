//! Finding Frames on the local network over mDNS.

use std::collections::HashMap;
use std::net::IpAddr;
use std::time::{Duration, Instant};

use mdns_sd::{ServiceDaemon, ServiceEvent};
use serde::Serialize;

use super::pairlink::join_addr;
use super::tls::normalize_fingerprint;

pub const SERVICE: &str = "_framecorder._tcp.local.";

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

/// Looks for one particular Frame, e.g. after its address changed.
pub async fn find(fingerprint: &str, window: Duration) -> Option<Found> {
    let mut hit = None;
    let _ = browse(window, |f| {
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

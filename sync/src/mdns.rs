//! Tells the network we're here as `_framecorder._tcp.local.`

use mdns_sd::{IfKind, ServiceDaemon, ServiceInfo};

pub const SERVICE: &str = "_framecorder._tcp.local.";
/// How often mdns-sd looks for new or changed network interfaces. Waking up
/// from standby brings Wi-Fi back, this is how long until we notice.
const IP_CHECK_SECS: u32 = 15;

pub fn advertise(name: &str, fingerprint: &str, port: u16) -> Result<ServiceDaemon, mdns_sd::Error> {
    let mdns = ServiceDaemon::new()?;
    mdns.disable_interface(IfKind::IPv6)?;
    mdns.disable_interface(IfKind::LoopbackV4)?;
    mdns.set_ip_check_interval(IP_CHECK_SECS)?;

    let short = &fingerprint[..8.min(fingerprint.len())];
    let instance = format!("{name} {short}");
    let host = format!("framecorder-{short}.local.");
    let props = [("fp", fingerprint), ("name", name), ("v", "1")];
    let info = ServiceInfo::new(SERVICE, &instance, &host, "", port, &props[..])?.enable_addr_auto();
    mdns.register(info)?;
    Ok(mdns)
}

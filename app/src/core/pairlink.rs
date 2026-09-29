//! The link in the Frame's pairing QR code:
//! `framecorder://pair?host=<ip>&port=<port>&fp=<sha256>&code=<code>&name=<name>`

use reqwest::Url;

use super::tls::normalize_fingerprint;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PairLink {
    pub addr: String,
    pub fingerprint: String,
    pub code: String,
    pub name: String,
}

pub fn parse(link: &str) -> Result<PairLink, String> {
    let url = Url::parse(link.trim()).map_err(|_| "that isn't a framecorder pairing link".to_string())?;
    if url.scheme() != "framecorder" || url.host_str() != Some("pair") {
        return Err("that isn't a framecorder pairing link".into());
    }
    let get = |key: &str| url.query_pairs().find(|(k, _)| k == key).map(|(_, v)| v.trim().to_string());
    let host = get("host").filter(|h| !h.is_empty()).ok_or("the link has no host")?;
    let port: u16 = get("port").and_then(|p| p.parse().ok()).ok_or("the link has no port")?;
    let fingerprint = get("fp").and_then(|f| normalize_fingerprint(&f)).ok_or("the link has no valid fingerprint")?;
    let code = get("code").filter(|c| is_code(c)).ok_or("the link has no pairing code")?;
    let name = get("name").filter(|n| !n.is_empty()).unwrap_or_else(|| "Steam Frame".into());
    Ok(PairLink { addr: join_addr(&host, port), fingerprint, code, name })
}

pub fn is_code(code: &str) -> bool {
    code.len() == 6 && code.bytes().all(|b| b.is_ascii_digit())
}

/// `host:port`, with brackets for IPv6.
pub fn join_addr(host: &str, port: u16) -> String {
    if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]:{port}")
    } else {
        format!("{host}:{port}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_qr_link() {
        let fp = "a1".repeat(32);
        let link = format!("framecorder://pair?host=192.168.1.20&port=38619&fp={fp}&code=123456&name=Sam%27s%20Frame");
        let p = parse(&link).unwrap();
        assert_eq!(p.addr, "192.168.1.20:38619");
        assert_eq!(p.fingerprint, fp);
        assert_eq!(p.code, "123456");
        assert_eq!(p.name, "Sam's Frame");
    }

    #[test]
    fn rejects_broken_links() {
        let fp = "a1".repeat(32);
        assert!(parse("https://example.com/pair?host=1").is_err());
        assert!(parse(&format!("framecorder://pair?host=1.2.3.4&port=1&fp={fp}&code=12345")).is_err());
        assert!(parse(&format!("framecorder://pair?host=1.2.3.4&port=x&fp={fp}&code=123456")).is_err());
        assert!(parse("framecorder://pair?host=1.2.3.4&port=1&fp=nope&code=123456").is_err());
        let ok = parse(&format!("framecorder://pair?host=fe80::1&port=1&fp={fp}&code=123456")).unwrap();
        assert_eq!(ok.addr, "[fe80::1]:1");
        assert_eq!(ok.name, "Steam Frame");
    }
}

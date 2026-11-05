//! What every report leaves out, whoever builds it: IP addresses, email
//! addresses, Steam IDs and the names devices paired with. The headset, the
//! apps and the relay all `#[path]`-include this file, so they agree.

use regex::Regex;
use std::net::IpAddr;
use std::sync::LazyLock;

pub fn hide(text: &str) -> String {
    static EMAIL: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*\.[A-Za-z]{2,}\b").expect("email pattern"));
    static STEAM_ID: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\b7656119\d{10}\b").expect("steam id pattern"));
    // the sync service used to log the name a device paired with ("Cole's phone")
    static PAIRED: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"paired (?:"[^"]*"|\\"(?:[^\\"]|\\[^"])*\\")"#).expect("paired pattern"));
    let text = EMAIL.replace_all(text, "<email>");
    let text = STEAM_ID.replace_all(&text, "<steam id>");
    let text = PAIRED.replace_all(&text, "paired <device>");
    hide_addresses(&text)
}

/// Default names every install has, hiding them would only blank out words.
const GENERIC: [&str; 6] = ["desktop", "localhost", "steam frame", "steamdeck", "steamos", "android"];

/// Names the caller knows are private (the device's own name, what it paired
/// with, a Steam name), wherever they turn up as whole words.
pub fn hide_names(text: &str, names: &[String]) -> String {
    let mut text = text.to_string();
    // longest first, so "Cole's S25 Ultra" goes before "Cole"
    let mut names: Vec<&String> = names.iter().filter(|n| n.trim().chars().count() >= 3 && !GENERIC.contains(&n.to_lowercase().as_str())).collect();
    names.sort_by_key(|n| std::cmp::Reverse(n.len()));
    for name in names {
        // whole words only: a Steam name "coah" mustn't eat "coah80.framecorder"
        let edge = |c: Option<char>| if c.is_some_and(|c| c.is_alphanumeric() || c == '_') { r"\b" } else { "" };
        let name = name.trim();
        let pattern = format!("{}{}{}", edge(name.chars().next()), regex::escape(name), edge(name.chars().last()));
        if let Ok(re) = Regex::new(&pattern) {
            text = re.replace_all(&text, "<name>").into_owned();
        }
    }
    text
}

/// What splits text into words that might be an address. `:` and `.` aren't
/// in it, they're inside addresses; `[` and `]` are, so `[::1]:80` works.
const DELIMITERS: &[char] = &[
    ' ', '\t', '\n', '\r', '"', '\'', '`', ',', ';', '(', ')', '<', '>', '=', '/', '@', '|', '{', '}', '[', ']',
];

/// Every address becomes `<ip>`, LAN ones included. Only loopback and
/// "any address" stay, they say nothing about anyone. It goes word by word,
/// not by pattern: a pattern loose enough for IPv6 also eats `sync::server`
/// and `trackpad.position`.
fn hide_addresses(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        let end = rest.find(DELIMITERS).unwrap_or(rest.len());
        let (word, tail) = rest.split_at(end);
        hide_word(word, &mut out);
        let delimiter = tail.chars().next().map_or(0, char::len_utf8);
        out.push_str(&tail[..delimiter]);
        rest = &tail[delimiter..];
    }
    out
}

fn hide_word(word: &str, out: &mut String) {
    // a sentence's full stop or a trailing "addr:" isn't part of it, unless
    // it's an address that ends in "::"
    let core = if is_address(word) { word } else { word.trim_end_matches(['.', ':']) };
    let after = &word[core.len()..];
    // 10.0.0.2:38619, the port can stay
    let (address, port) = match core.rsplit_once(':') {
        Some((ip, port)) if !port.is_empty() && port.bytes().all(|b| b.is_ascii_digit()) && ip.parse::<std::net::Ipv4Addr>().is_ok() => (ip, &core[ip.len()..]),
        _ => (core, ""),
    };
    // fe80::1%wlan0, the interface's name says nothing
    let (bare, zone) = address.split_once('%').unwrap_or((address, ""));
    match bare.parse::<IpAddr>() {
        Ok(ip) if is_address(bare) && !ip.is_loopback() && !ip.is_unspecified() => {
            out.push_str("<ip>");
            out.push_str(port);
            if !zone.is_empty() {
                out.push('%');
                out.push_str(zone);
            }
            out.push_str(after);
        }
        _ => out.push_str(word),
    }
}

/// Parses as an address and has a digit in it: `c::` and `dead::beef` are
/// valid IPv6 but turn up in logs as code, real addresses have numbers.
fn is_address(word: &str) -> bool {
    word.bytes().any(|b| b.is_ascii_digit()) && word.split('%').next().is_some_and(|w| w.parse::<IpAddr>().is_ok())
}

#[cfg(test)]
mod tests {
    use super::{hide, hide_names};

    #[test]
    fn every_address_goes_and_nothing_else_does() {
        for (input, expected) in [
            ("8.8.8.8 192.168.1.2 10.0.0.2 172.16.0.1 169.254.1.1", "<ip> <ip> <ip> <ip> <ip>"),
            ("http://172.20.1.123:38619/v1 at 10.1.2.3.", "http://<ip>:38619/v1 at <ip>."),
            ("[2001:4860:4860::8888]:443 fe80::1%wlan0 fd00::1", "[<ip>]:443 <ip>%wlan0 <ip>"),
            ("127.0.0.1:4796 [::1] 0.0.0.0 ::", "127.0.0.1:4796 [::1] 0.0.0.0 ::"),
            ("addr=192.168.0.5,peer=\"10.0.0.9\"", "addr=<ip>,peer=\"<ip>\""),
            // what a pattern used to mangle
            ("framecorder_sync::server trackpad.position c:: 23:00:11 v1.2.3 4320x2160@90.0Hz", "framecorder_sync::server trackpad.position c:: 23:00:11 v1.2.3 4320x2160@90.0Hz"),
            ("mail cole@example.com at 4320x2160@90.000008Hz", "mail <email> at 4320x2160@90.000008Hz"),
            ("steam 76561198012345678 signed in", "steam <steam id> signed in"),
            (r#"INFO framecorder_sync::server] paired "Cole's S25 Ultra" as b438"#, "INFO framecorder_sync::server] paired <device> as b438"),
            (r#"{"line":"paired \"Sam's MacBook\" as 12"}"#, r#"{"line":"paired <device> as 12"}"#),
        ] {
            assert_eq!(hide(input), expected, "{input}");
        }
        let names = ["Cole".to_string(), "Cole's S25 Ultra".into(), "".into(), "PC".into(), "Steam Frame".into(), "coah".into()];
        assert_eq!(
            hide_names("Cole's S25 Ultra and Cole on a PC with a Steam Frame, coah as coah80.framecorder", &names),
            "<name> and <name> on a PC with a Steam Frame, <name> as coah80.framecorder"
        );
    }
}

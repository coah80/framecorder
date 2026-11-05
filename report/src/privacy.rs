use regex::{Captures, Regex};
use std::{net::IpAddr, sync::LazyLock};

fn pattern(s: &str) -> Regex {
    Regex::new(s).expect("valid redaction pattern")
}

pub fn scrub(input: &str) -> String {
    static PEM: LazyLock<Regex> =
        LazyLock::new(|| pattern(r#"(?s)-----BEGIN [^-]+-----.*?(?:-----END [^-]+-----|$)"#));
    static HOME: LazyLock<Regex> = LazyLock::new(|| {
        pattern(r#"(?:/(?:home|Users)/[^/\s\"'<>]+|[A-Za-z]:\\Users\\[^\\\s\"'<>]+)"#)
    });
    static NAMED: LazyLock<Regex> = LazyLock::new(|| {
        pattern(
            r#"(?i)\b(token|password|passwd|secret|pairing[_ -]?(?:code|token)|authorization|api[_ -]?key|serial(?:[_ -]?(?:number|no))?|user(?:name)?|login|account(?:[_ -]?name)?)"?\s*[:=]\s*(?:Bearer\s+)?(?:\"[^\"\r\n]*\"|[^\s,;]+)"#,
        )
    });
    static MAC: LazyLock<Regex> = LazyLock::new(|| {
        pattern(r#"(?i)\b[0-9a-f]{2}(?::[0-9a-f]{2}){5}\b|\b[0-9a-f]{2}(?:-[0-9a-f]{2}){5}\b"#)
    });
    static SERIAL: LazyLock<Regex> =
        LazyLock::new(|| pattern(r#"(?i)\b(?:LHR|FPR|HMD|CTL)-[A-Za-z0-9-]+\b"#));
    static SECRET: LazyLock<Regex> = LazyLock::new(|| {
        pattern(
            r#"\b(?:[A-Fa-f0-9]{32,}|[A-Za-z0-9_+/=-]{40,}|gh[pousr]_[A-Za-z0-9_]+|github_pat_[A-Za-z0-9_]+)\b"#,
        )
    });
    static IPV4: LazyLock<Regex> =
        LazyLock::new(|| pattern(r#"\b(?:[0-9]{1,3}\.){3}[0-9]{1,3}\b"#));
    static IPV6: LazyLock<Regex> = LazyLock::new(|| pattern(r#"[0-9A-Fa-f:]*:[0-9A-Fa-f:.%]+"#));
    let mut out = PEM.replace_all(input, "<secret>").into_owned();
    let users: Vec<String> = HOME
        .find_iter(&out)
        .filter_map(|m| m.as_str().rsplit(['/', '\\']).next().map(str::to_owned))
        .collect();
    out = HOME.replace_all(&out, "~").into_owned();
    for user in users {
        // steamos's own user names are on every headset, they identify nobody
        if !user.is_empty() && !["steamos", "deck"].contains(&user.as_str()) {
            out = pattern(&format!(r"\b{}\b", regex::escape(&user)))
                .replace_all(&out, "<user>")
                .into_owned();
        }
    }
    out = NAMED
        .replace_all(&out, |c: &Captures<'_>| format!("{}=<redacted>", &c[1]))
        .into_owned();
    out = MAC.replace_all(&out, "<mac>").into_owned();
    out = SERIAL.replace_all(&out, "<serial>").into_owned();
    out = SECRET.replace_all(&out, "<secret>").into_owned();
    for re in [&*IPV4, &*IPV6] {
        out = re
            .replace_all(&out, |c: &Captures<'_>| match c[0].parse::<IpAddr>() {
                Ok(IpAddr::V4(ip)) if ip.is_private() || ip.is_loopback() || ip.is_link_local() => {
                    c[0].to_owned()
                }
                Ok(IpAddr::V6(ip))
                    if ip.is_loopback() || ip.is_unique_local() || ip.is_unicast_link_local() =>
                {
                    c[0].to_owned()
                }
                Ok(_) => "<ip>".to_owned(),
                Err(_) => c[0].to_owned(),
            })
            .into_owned();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn private_data_is_removed_but_lan_and_persona_stay() {
        for (input, expected) in [
            ("/home/alice/log alice", "~/log <user>"),
            ("/home/steamos/log on steamos", "~/log on steamos"),
            (r"C:\Users\alice\log", r"~\log"),
            ("/Users/alice/log", "~/log"),
            (
                "8.8.8.8 192.168.1.2 172.31.1.2 172.32.1.2 10.1.2.3",
                "<ip> 192.168.1.2 172.31.1.2 <ip> 10.1.2.3",
            ),
            ("2001:4860:4860::8888 ::1 fd00::1", "<ip> ::1 fd00::1"),
            ("aa:bb:cc:dd:ee:ff AA-BB-CC-DD-EE-FF", "<mac> <mac>"),
            (
                "token=short serial: ABC123 login=alice",
                "token=<redacted> serial=<redacted> login=<redacted>",
            ),
            ("LHR-12345", "<serial>"),
            (
                "Authorization: Bearer short-token",
                "Authorization=<redacted>",
            ),
            (r#"{"token":"tiny-secret"}"#, r#"{"token=<redacted>}"#),
            (
                "username: alice account=alice serialnumber=1234",
                "username=<redacted> account=<redacted> serialnumber=<redacted>",
            ),
            (
                "-----BEGIN RSA PRIVATE KEY-----\nabc\n-----END RSA PRIVATE KEY-----",
                "<secret>",
            ),
            ("0123456789abcdef0123456789abcdef", "<secret>"),
            ("Steam name: Moon Walker", "Steam name: Moon Walker"),
        ] {
            assert_eq!(scrub(input), expected, "{input}");
        }
    }
}

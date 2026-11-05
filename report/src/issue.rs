use crate::privacy::scrub;
use serde::Deserialize;
use serde_json::Value;

pub const MAX_REQUEST: usize = 1024 * 1024;
const BODY_LIMIT: usize = 65536;

#[derive(Deserialize)]
pub struct Log {
    pub name: String,
    pub text: String,
}

#[derive(Deserialize)]
pub struct Report {
    pub version: u32,
    pub source: String,
    pub description: String,
    pub app: Option<Value>,
    pub headset: Option<Value>,
    pub headset_unavailable: Option<String>,
    #[serde(default)]
    pub app_logs: Vec<Log>,
}

pub fn parse(bytes: &[u8]) -> Result<Report, &'static str> {
    let mut report: Report = serde_json::from_slice(bytes)
        .map_err(|_| "The report could not be read. Try sending it again.")?;
    report.description = report.description.trim().to_owned();
    if report.version != 1 || !["headset", "desktop", "android"].contains(&report.source.as_str()) {
        return Err("This report format is not supported. Update framecorder and try again.");
    }
    if !(1..=4000).contains(&report.description.chars().count()) {
        return Err("Describe what happened in 1 to 4000 characters.");
    }
    if report.app.as_ref().is_some_and(|v| !v.is_object())
        || report.headset.as_ref().is_some_and(|v| !v.is_object())
    {
        return Err("The report details could not be read. Try sending it again.");
    }
    Ok(report)
}

fn neutralize(s: &str) -> String {
    scrub(s).replace('@', "@\u{200b}").replace('#', "#\u{200b}")
}

fn escaped(s: &str) -> String {
    neutralize(s)
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('|', "&#124;")
        .replace(['\r', '\n'], " ")
        .replace('`', "&#96;")
        .replace('*', "&#42;")
        .replace('_', "&#95;")
        .replace('[', "&#91;")
        .replace(']', "&#93;")
        .replace('\\', "&#92;")
}

fn field(value: &Value, path: &str) -> String {
    value
        .pointer(path)
        .filter(|v| !v.is_null())
        .map(|v| match v {
            Value::String(s) => s.to_owned(),
            Value::Array(items) => items
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .unwrap_or_else(|| v.to_string())
                })
                .collect::<Vec<_>>()
                .join(", "),
            _ => v.to_string(),
        })
        .unwrap_or_else(|| "unknown".to_owned())
}

fn tail(s: &str, budget: usize) -> String {
    s.chars()
        .rev()
        .take(budget)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn block(s: &str) -> String {
    format!("```text\n{}\n```\n", s.replace('`', "ˋ"))
}

pub fn build(report: &Report) -> (String, String) {
    let title = format!(
        "[{}] {}",
        report.source,
        neutralize(report.description.lines().next().unwrap_or_default())
    )
    .chars()
    .filter(|c| !c.is_control())
    .take(80)
    .collect();
    let description = neutralize(&report.description)
        .chars()
        .fold(String::new(), |mut out, c| {
            if "\\`*_[]!(){}".contains(c) {
                out.push('\\');
            }
            out.push(c);
            out
        })
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    let mut body = description
        .lines()
        .map(|line| format!("> {line}\n"))
        .collect::<String>();
    let h = report.headset.as_ref().unwrap_or(&Value::Null);
    let a = report.app.as_ref().unwrap_or(&Value::Null);
    body.push_str("\n| Detail | Value |\n| --- | --- |\n");
    for (name, value) in [
        ("Source", report.source.clone()),
        (
            "framecorder",
            format!(
                "{} / {}",
                field(h, "/framecorder/version"),
                field(h, "/framecorder/channel")
            ),
        ),
        (
            "App",
            format!(
                "{} {} / {} / {}",
                field(a, "/name"),
                field(a, "/version"),
                field(a, "/os"),
                field(a, "/arch")
            ),
        ),
        (
            "SteamOS",
            format!(
                "{} / {} / {}",
                field(h, "/os/version"),
                field(h, "/os/build_id"),
                field(h, "/os/branch")
            ),
        ),
        (
            "SteamVR",
            format!(
                "{} / {}",
                field(h, "/steam/steamvr_version"),
                field(h, "/steam/steamvr_branch")
            ),
        ),
        (
            "Device",
            format!(
                "{} / {} / {}",
                field(h, "/device/model"),
                field(h, "/device/cpu"),
                field(a, "/device")
            ),
        ),
        (
            "RAM",
            format!(
                "{} MiB available / {} MiB total",
                field(h, "/device/ram_available_mb"),
                field(h, "/device/ram_total_mb")
            ),
        ),
        (
            "Disk free",
            format!("{} MiB", field(h, "/device/disk_free_mb")),
        ),
        (
            "Paired apps",
            format!("{} / {}", field(h, "/sync/paired"), field(h, "/sync/apps")),
        ),
        (
            "Headset unavailable",
            report
                .headset_unavailable
                .clone()
                .unwrap_or_else(|| "no".to_owned()),
        ),
    ] {
        body.push_str(&format!(
            "| {name} | {} |\n",
            escaped(&value).chars().take(512).collect::<String>()
        ));
    }
    body.push_str("\n### Recent errors\n\n");
    let errors = h
        .get("recent_errors")
        .and_then(Value::as_array)
        .map(|lines| {
            lines
                .iter()
                .filter_map(Value::as_str)
                .map(neutralize)
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();

    let mut logs: Vec<(String, String)> = h
        .get("logs")
        .and_then(Value::as_array)
        .map(|logs| {
            logs.iter()
                .filter_map(|l| {
                    Some((
                        l.get("name")?.as_str()?.to_owned(),
                        neutralize(l.get("text")?.as_str()?),
                    ))
                })
                .collect()
        })
        .unwrap_or_default();
    logs.extend(
        report
            .app_logs
            .iter()
            .map(|l| (l.name.clone(), neutralize(&l.text))),
    );
    let error_budget = BODY_LIMIT.saturating_sub(body.chars().count() + 128);
    body.push_str(&block(&tail(&errors, error_budget)));
    if errors.chars().count() > error_budget {
        body.push_str("\nOlder recent errors omitted to fit GitHub's limit.\n");
    }
    let available = BODY_LIMIT.saturating_sub(body.chars().count() + 128);
    let mut frames = Vec::new();
    let mut overhead = 0;
    for (name, _) in &logs {
        let frame = format!(
            "\n<details>\n<summary>{}</summary>\n\n",
            escaped(name).chars().take(128).collect::<String>()
        );
        if overhead + frame.chars().count() + 40 > available {
            break;
        }
        overhead += frame.chars().count() + 40;
        frames.push(frame);
    }
    let omitted = logs.len() - frames.len();
    let share = available.saturating_sub(overhead) / frames.len().max(1);
    for ((_, text), frame) in logs.iter().zip(frames) {
        body.push_str(&frame);
        let text = tail(text, share);
        body.push_str(&block(&text));
        body.push_str("\n</details>\n");
    }
    if omitted > 0 {
        body.push_str(&format!(
            "\n{omitted} logs omitted to fit GitHub's limit.\n"
        ));
    }
    (title, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn validates_contract() {
        for (source, version, description, valid) in [
            ("headset", 1, " ok ", true),
            ("desktop", 1, "🦀", true),
            ("android", 1, "", false),
            ("headset", 1, " \n ", false),
            ("web", 1, "ok", false),
            ("headset", 2, "ok", false),
        ] {
            assert_eq!(
                parse(
                    &serde_json::to_vec(
                        &json!({"source":source,"version":version,"description":description})
                    )
                    .unwrap()
                )
                .is_ok(),
                valid
            );
        }
        assert!(parse(br#"{}"#).is_err());
        assert!(parse(
            &serde_json::to_vec(
                &json!({"source":"headset","version":1,"description":"a".repeat(4001)})
            )
            .unwrap()
        )
        .is_err());
    }
    #[test]
    fn errors_take_priority_and_pathological_logs_stay_in_budget() {
        for (errors, logs) in [
            (
                "warning failed\n".repeat(2500),
                vec![json!({"name":"recorder.log","text":"older log\n".repeat(15000) + "NEWEST"})],
            ),
            (
                "`".repeat(100000),
                (0..16)
                    .map(|_| json!({"name":"<".repeat(200),"text":"`".repeat(100000)}))
                    .collect(),
            ),
        ] {
            let r = parse(&serde_json::to_vec(&json!({"source":"desktop","version":1,"description":"![image](https://example.com)\n".repeat(10),"headset":{"recent_errors":[errors],"logs":logs}})).unwrap()).unwrap();
            let (_, body) = build(&r);
            assert!(
                body.chars().count() <= BODY_LIMIT,
                "{}",
                body.chars().count()
            );
            assert!(body.contains(r"\!\[image\]\(https://example.com\)"));
            if errors.starts_with("warning") {
                assert!(body.contains(&errors));
                assert!(body.contains("NEWEST"));
            }
        }
    }

    #[test]
    fn hostile_markdown_and_budget_keep_diagnostics_readable() {
        let r = parse(&serde_json::to_vec(&json!({"source":"headset","version":1,"description":"@someone #42\n</blockquote><script>oops</script>","headset":{"device":{"model":"a|b\n<img>"},"recent_errors":["recorder: failed to encode"],"logs":[{"name":"</summary><img>","text":format!("{}\n```\n</details>\n@evil #12\nEND", "older".repeat(20000))}]},"app_logs":[{"name":"app","text":"z".repeat(100000)}]})).unwrap()).unwrap();
        let (title, body) = build(&r);
        assert!(title.chars().count() <= 80);
        assert!(body.chars().count() <= BODY_LIMIT);
        assert!(!body.contains("@someone"));
        assert!(!body.contains("#42"));
        assert!(body.contains("&lt;script&gt;"));
        assert!(body.contains("a&#124;b &lt;img&gt;"));
        assert!(!body.contains("Steam name"));
        assert!(body.contains("recorder: failed to encode"));
        assert!(body.contains("ˋˋˋ\n</details>"));
        assert!(body.contains("\nEND\n```"));
    }
}

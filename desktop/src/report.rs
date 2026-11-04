//! report an issue: say what went wrong, see what goes with it, and a github
//! issue opens with the lot. the collecting and sending are the app core's
//! (`framecorder_app_lib::core::report`), this is the screen and its steps.

use std::sync::Arc;

use framecorder_app_lib::core::report::{self as core, AppInfo, Report, Sent};
use gpui::{
    div, prelude::*, px, rgb_to_hsla, AnyElement, ClipboardItem, Context, Div, FocusHandle, FontWeight, HighlightStyle,
    KeyDownEvent, SharedString, StyledText, Task, Window,
};

use crate::app::{FrameApp, Page};
use crate::theme::{self, a, c};
use crate::widgets::{card, data, icon, label, outline, primary, spinner, title};

/// the relay takes 1 to 4000 characters
pub const MOST: usize = 4000;
const ISSUES: &str = "github.com/coah80/framecorder";

pub enum Phase {
    Writing,
    /// asking the frame and this app for their diagnostics
    Collecting,
    /// everything's gathered and shown, waiting for the go
    Review { json: bool },
    Sending,
    Sent(Sent),
    Failed(String),
}

pub struct State {
    pub description: String,
    pub focus: FocusHandle,
    /// what's shown and then sent, exactly as collected
    pub preview: Option<Arc<Report>>,
    pretty: Option<SharedString>,
    pub phase: Phase,
    task: Option<Task<()>>,
}

impl State {
    pub fn new(focus: FocusHandle) -> Self {
        Self { description: String::new(), focus, preview: None, pretty: None, phase: Phase::Writing, task: None }
    }

    /// the full json, made once per preview
    fn json(&mut self) -> SharedString {
        if let Some(p) = &self.pretty {
            return p.clone();
        }
        let text = self.preview.as_ref().and_then(|r| serde_json::to_string_pretty(r.as_ref()).ok()).unwrap_or_default();
        self.pretty = Some(text.into());
        self.pretty.clone().unwrap_or_default()
    }

    pub fn show(&mut self, preview: Report, json: bool) {
        self.preview = Some(Arc::new(preview));
        self.pretty = None;
        self.phase = Phase::Review { json };
    }
}

/// adds what was typed or pasted, up to the limit, dropping control characters
pub fn push_typed(description: &mut String, text: &str) {
    let room = MOST.saturating_sub(description.chars().count());
    description.extend(text.chars().filter(|c| !c.is_control() || *c == '\n').take(room));
}

/// this app, for the report: the version, the os, and a hostname-free word for the machine
pub fn app_info() -> AppInfo {
    AppInfo { device: device(), ..AppInfo::current("framecorder desktop", env!("CARGO_PKG_VERSION")) }
}

fn device() -> String {
    #[cfg(target_os = "linux")]
    {
        let display = if std::env::var_os("WAYLAND_DISPLAY").is_some() { "wayland" } else { "x11" };
        let packaging = if std::env::var_os("APPIMAGE").is_some() { "appimage" } else { "built from source" };
        format!("linux pc, {display}, {packaging}")
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("sysctl")
            .args(["-n", "hw.model"])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|m| !m.is_empty())
            .unwrap_or_else(|| "mac".into())
    }
    #[cfg(windows)]
    {
        "windows pc".into()
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        "computer".into()
    }
}

/// one line of the "what gets sent" list
pub struct Line {
    pub name: &'static str,
    pub value: String,
}

fn line(name: &'static str, value: impl Into<String>) -> Line {
    Line { name, value: value.into() }
}

/// what's in a report, in plain words, for the review step
pub fn summary(r: &Report) -> Vec<Line> {
    let mut out = vec![line("this app", format!("{} {} · {} · {} · {}", r.app.name, r.app.version, r.app.os, r.app.device, r.app.arch))];
    let str_at = |v: &serde_json::Value, path: &[&str]| -> Option<String> {
        let mut cur = v;
        for key in path {
            cur = cur.get(key)?;
        }
        match cur {
            serde_json::Value::String(s) => Some(s.clone()),
            serde_json::Value::Null => None,
            other => Some(other.to_string()),
        }
    };
    match (&r.headset, &r.headset_unavailable) {
        (Some(h), None) => {
            let version = str_at(h, &["framecorder", "version"]).unwrap_or_else(|| "unknown".into());
            let channel = str_at(h, &["framecorder", "channel"]).map(|c| format!(" ({c})")).unwrap_or_default();
            let os = [str_at(h, &["os", "name"]), str_at(h, &["os", "version"]), str_at(h, &["os", "build_id"])]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
            let vr = [str_at(h, &["steam", "steamvr_version"]), str_at(h, &["steam", "steamvr_branch"])]
                .into_iter()
                .flatten()
                .collect::<Vec<_>>()
                .join(" ");
            let mut about = vec![format!("framecorder {version}{channel}")];
            if !os.is_empty() {
                about.push(os);
            }
            if !vr.is_empty() {
                about.push(format!("steamvr {vr}"));
            }
            out.push(line("the headset", about.join(" · ")));
            out.push(line("steam name", str_at(h, &["steam", "name"]).unwrap_or_else(|| "not signed in, so not included".into())));
            let errors = h.get("recent_errors").and_then(|e| e.as_array()).map_or(0, Vec::len);
            out.push(line("recent errors", match errors {
                0 => "none in the last 48 hours".to_string(),
                1 => "1 line from the last 48 hours".to_string(),
                n => format!("{n} lines from the last 48 hours"),
            }));
            let logs: Vec<String> = h
                .get("logs")
                .and_then(|l| l.as_array())
                .map(|l| l.iter().filter_map(|x| str_at(x, &["name"])).collect())
                .unwrap_or_default();
            if !logs.is_empty() {
                out.push(line("its logs", logs.join(", ")));
            }
        }
        (_, Some(why)) => out.push(line("the headset", format!("not included: {why}"))),
        (None, None) => out.push(line("the headset", "not included")),
    }
    let app_logs: Vec<String> = r.app_logs.iter().map(|l| format!("{} ({})", l.name, crate::format::size(l.text.len() as u64))).collect();
    out.push(line("this app's logs", if app_logs.is_empty() { "none".into() } else { app_logs.join(", ") }));
    out
}

impl FrameApp {
    pub fn open_report(&mut self, cx: &mut Context<Self>) {
        if matches!(self.report.phase, Phase::Sent(_)) {
            self.report_reset();
        }
        self.go(Page::Report, cx);
    }

    fn report_reset(&mut self) {
        self.report.description.clear();
        self.report.preview = None;
        self.report.pretty = None;
        self.report.phase = Phase::Writing;
        self.report.task = None;
    }

    /// typing in the description. enter is a new line, esc leaves the box
    pub fn report_key(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(self.report.phase, Phase::Writing) {
            return;
        }
        let k = &ev.keystroke;
        let shortcut = k.modifiers.control || k.modifiers.platform;
        let d = &mut self.report.description;
        if shortcut && k.key == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                push_typed(d, &text);
            }
        } else if shortcut && k.key == "backspace" {
            d.clear();
        } else if k.key == "backspace" {
            d.pop();
        } else if k.key == "enter" {
            push_typed(d, "\n");
        } else if k.key == "escape" {
            window.blur();
        } else if let Some(ch) = k.key_char.as_deref().filter(|_| !shortcut) {
            push_typed(d, ch);
        } else {
            return;
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub fn report_can_collect(&self) -> bool {
        matches!(self.report.phase, Phase::Writing) && !self.report.description.trim().is_empty()
    }

    /// gathers everything that goes with the description, to show it first
    pub fn report_collect(&mut self, cx: &mut Context<Self>) {
        if !self.report_can_collect() {
            return;
        }
        self.report.phase = Phase::Collecting;
        cx.notify();
        let description = self.report.description.clone();
        if self.demo {
            self.report.task = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(900)).await;
                let _ = this.update(cx, |app, cx| {
                    app.report.show(crate::demo::report(&description), false);
                    cx.notify();
                });
            }));
            return;
        }
        let engine = self.core.engine.clone();
        let job = self.core.rt.spawn(async move { engine.report_preview(&description, app_info()).await });
        self.report.task = Some(cx.spawn(async move |this, cx| {
            let res = job.await;
            let _ = this.update(cx, |app, cx| {
                match res {
                    Ok(report) => app.report.show(report, false),
                    Err(e) => app.report.phase = Phase::Failed(format!("couldn't put the report together: {e}")),
                }
                cx.notify();
            });
        }));
    }

    pub fn report_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.report.phase = Phase::Writing;
        self.report.task = None;
        self.report.focus.focus(window, cx);
        cx.notify();
    }

    pub fn report_toggle_json(&mut self, cx: &mut Context<Self>) {
        if let Phase::Review { json } = &mut self.report.phase {
            *json = !*json;
            cx.notify();
        }
    }

    /// sends exactly what was shown
    pub fn report_send(&mut self, cx: &mut Context<Self>) {
        let Some(report) = self.report.preview.clone() else { return };
        if !matches!(self.report.phase, Phase::Review { .. } | Phase::Failed(_)) {
            return;
        }
        self.report.phase = Phase::Sending;
        cx.notify();
        if self.demo {
            self.report.task = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(1200)).await;
                let _ = this.update(cx, |app, cx| {
                    app.report.phase =
                        Phase::Sent(Sent { number: 128, url: format!("https://{ISSUES}/issues/128") });
                    cx.notify();
                });
            }));
            return;
        }
        let job = self.core.rt.spawn(async move { core::send(&report).await });
        self.report.task = Some(cx.spawn(async move |this, cx| {
            let res = job.await.unwrap_or_else(|e| Err(core::ReportError(format!("couldn't send the report: {e}"))));
            let _ = this.update(cx, |app, cx| {
                app.report.phase = match res {
                    Ok(sent) => Phase::Sent(sent),
                    Err(e) => Phase::Failed(e.0),
                };
                cx.notify();
            });
        }));
    }

    pub fn report_open_issue(&mut self, cx: &mut Context<Self>) {
        if let Phase::Sent(s) = &self.report.phase {
            cx.open_url(&s.url);
        }
    }

    pub fn report_copy_link(&mut self, cx: &mut Context<Self>) {
        if let Phase::Sent(s) = &self.report.phase {
            cx.write_to_clipboard(ClipboardItem::new_string(s.url.clone()));
            self.toast("copied the link", cx);
        }
    }

    pub fn report_done(&mut self, cx: &mut Context<Self>) {
        self.report_reset();
        self.go(Page::Clips, cx);
    }
}

// the page

pub fn render(app: &mut FrameApp, window: &mut Window, cx: &mut Context<FrameApp>) -> impl IntoElement {
    // the box is the only thing to type in, so it has the keyboard
    if matches!(app.report.phase, Phase::Writing) && window.focused(cx).is_none() {
        app.report.focus.focus(window, cx);
    }
    let body: AnyElement = match &app.report.phase {
        Phase::Writing => writing(app, window, cx).into_any_element(),
        Phase::Collecting => collecting().into_any_element(),
        Phase::Review { json } => review(app, *json, false, cx).into_any_element(),
        Phase::Sending => review(app, false, true, cx).into_any_element(),
        Phase::Sent(sent) => sent_card(sent, cx).into_any_element(),
        Phase::Failed(why) => failed(app, why.clone(), cx).into_any_element(),
    };
    div()
        .id("report-page")
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_y_scroll()
        .px(px(28.))
        .pt(px(26.))
        .pb(px(32.))
        .flex()
        .flex_col()
        .gap(px(18.))
        .child(div().flex().flex_col().gap(px(6.)).child(title("report an issue", 26.)).child(
            div().text_size(px(13.)).text_color(c(theme::SUBTEXT0)).child(
                "say what went wrong, and it goes to github with what's needed to work it out, so we can fix it and tell you.",
            ),
        ))
        .child(body)
}

fn writing(app: &FrameApp, window: &mut Window, cx: &mut Context<FrameApp>) -> Div {
    let d = &app.report.description;
    let focused = app.report.focus.is_focused(window);
    let empty = d.is_empty();
    let count = d.chars().count();
    div()
        .flex()
        .flex_col()
        .gap(px(14.))
        .child(
            card().flex().flex_col().gap(px(10.)).p(px(16.)).child(label("what went wrong")).child(
                div()
                    .id("report-text")
                    .track_focus(&app.report.focus)
                    .on_key_down(cx.listener(|app, ev, window, cx| app.report_key(ev, window, cx)))
                    .on_click(cx.listener(|app, _, window, cx| app.report.focus.focus(window, cx)))
                    .min_h(px(150.))
                    .p(px(14.))
                    .rounded(px(12.))
                    .bg(a(theme::CRUST, 0.35))
                    .border_1()
                    .border_color(if focused { c(theme::MAUVE) } else { theme::card_line() })
                    .cursor_text()
                    .text_size(px(14.))
                    .line_height(px(21.))
                    .whitespace_normal()
                    .child(if empty && !focused {
                        div().text_color(c(theme::OVERLAY2)).child(
                            "what you were doing, what you expected, and what happened instead. a clip that came out wrong? say which one.",
                        )
                    } else {
                        div().child(typed_text(d, focused))
                    }),
            )
            .child(
                div().flex().items_center().justify_between().child(
                    div().text_size(px(12.)).text_color(c(theme::OVERLAY2)).child("your steam name goes in, so we can reply where you'll see it."),
                )
                .child(data(format!("{count} / {MOST}")).text_size(px(11.)).text_color(c(if count >= MOST { theme::PEACH } else { theme::OVERLAY2 }))),
            ),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(12.))
                .child(if app.report_can_collect() {
                    primary("report-next", "see what gets sent")
                        .on_click(cx.listener(|app, _, _, cx| app.report_collect(cx)))
                } else {
                    primary("report-next", "see what gets sent").opacity(0.35).cursor_default()
                })
                .child(div().text_size(px(12.)).text_color(c(theme::SUBTEXT0)).child("nothing is sent until you've seen it.")),
        )
        .child(notice())
}

/// the description as typed, with a mauve caret on the end while the box has the keyboard
fn typed_text(text: &str, focused: bool) -> StyledText {
    if !focused {
        return StyledText::new(text.to_string());
    }
    let caret = HighlightStyle { color: Some(rgb_to_hsla(c(theme::MAUVE))), ..Default::default() };
    StyledText::new(format!("{text}|")).with_highlights([(text.len()..text.len() + 1, caret)])
}

/// the public note, on every step before sending
fn notice() -> Div {
    div()
        .flex()
        .items_start()
        .gap(px(12.))
        .p(px(16.))
        .rounded(px(14.))
        .bg(a(theme::MAUVE, 0.06))
        .child(icon("info", 18., c(theme::MAUVE)).mt(px(1.)))
        .child(div().flex_1().min_w_0().text_size(px(13.)).text_color(c(theme::SUBTEXT1)).child(format!(
            "it opens a public issue on {ISSUES}, so anyone can read it. file paths, addresses and anything secret are scrubbed before it leaves this computer, and nothing from your videos goes with it."
        )))
}

fn collecting() -> Div {
    card()
        .flex()
        .items_center()
        .gap(px(14.))
        .p(px(18.))
        .child(spinner("report-collecting", 18., c(theme::MAUVE)))
        .child(div().flex().flex_col().gap(px(2.)).child(div().font_weight(FontWeight::MEDIUM).child("putting it together")).child(
            div().text_size(px(12.)).text_color(c(theme::SUBTEXT0)).child("asking your frame how it's doing and gathering this app's log. a few seconds."),
        ))
}

fn review(app: &mut FrameApp, json: bool, sending: bool, cx: &mut Context<FrameApp>) -> Div {
    let Some(report) = app.report.preview.clone() else { return div() };
    let lines: Vec<AnyElement> = summary(&report)
        .into_iter()
        .enumerate()
        .map(|(i, l)| {
            div()
                .flex()
                .items_start()
                .gap(px(14.))
                .px(px(16.))
                .py(px(8.))
                .when(i > 0, |d| d.border_t_1().border_color(theme::line()))
                .child(div().w(px(128.)).flex_none().text_size(px(13.)).font_weight(FontWeight::MEDIUM).child(l.name))
                .child(div().flex_1().min_w_0().text_size(px(13.)).text_color(c(theme::SUBTEXT1)).whitespace_normal().child(l.value))
                .into_any_element()
        })
        .collect();
    let pretty = json.then(|| app.report.json());
    div()
        .flex()
        .flex_col()
        .gap(px(14.))
        .child(
            card()
                .flex()
                .flex_col()
                .child(div().px(px(16.)).pt(px(12.)).pb(px(4.)).child(label("what gets sent")))
                .child(
                    div()
                        .px(px(16.))
                        .py(px(10.))
                        .text_size(px(13.))
                        .text_color(c(theme::TEXT))
                        .whitespace_normal()
                        .line_height(px(20.))
                        .child(report.description.clone()),
                )
                .children(lines)
                .child(
                    div()
                        .id("report-json-toggle")
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .px(px(16.))
                        .py(px(10.))
                        .border_t_1()
                        .border_color(theme::line())
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(c(theme::MAUVE))
                        .cursor_pointer()
                        .hover(|s| s.text_color(c(theme::MAUVE_LIGHT)))
                        .on_click(cx.listener(|app, _, _, cx| app.report_toggle_json(cx)))
                        .child(if json { "hide the full report" } else { "see the full report, as json" }),
                )
                .when_some(pretty, |d, text| {
                    d.child(
                        div()
                            .id("report-json")
                            .max_h(px(320.))
                            .overflow_y_scroll()
                            .mx(px(16.))
                            .mb(px(16.))
                            .p(px(12.))
                            .rounded(px(10.))
                            .bg(a(theme::CRUST, 0.5))
                            .child(data(text).text_size(px(11.)).line_height(px(16.)).text_color(c(theme::SUBTEXT1)).whitespace_normal()),
                    )
                }),
        )
        .child(notice())
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(12.))
                .child(if sending {
                    primary("report-send", "sending").child(spinner("report-sending", 14., c(theme::CRUST))).opacity(0.7).cursor_default()
                } else {
                    primary("report-send", "send it to github").on_click(cx.listener(|app, _, _, cx| app.report_send(cx)))
                })
                .child(if sending {
                    outline("report-edit", "edit").opacity(0.35).cursor_default()
                } else {
                    outline("report-edit", "edit").on_click(cx.listener(|app, _, window, cx| app.report_edit(window, cx)))
                }),
        )
}

fn sent_card(sent: &Sent, cx: &mut Context<FrameApp>) -> Div {
    card()
        .flex()
        .flex_col()
        .gap(px(16.))
        .p(px(22.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(14.))
                .child(div().size(px(44.)).flex_none().flex().items_center().justify_center().rounded(px(12.)).bg(a(theme::GREEN, 0.14)).child(icon("check", 22., c(theme::GREEN))))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(div().font_family(theme::HEADING).font_weight(FontWeight::BOLD).text_size(px(20.)).child(format!("issue #{} is open", sent.number)))
                        .child(data(sent.url.clone()).text_size(px(12.)).text_color(c(theme::SUBTEXT0)).truncate()),
                ),
        )
        .child(div().text_size(px(13.)).text_color(c(theme::SUBTEXT1)).child(
            "thanks. keep an eye on it: that's where any questions and the fix will be.",
        ))
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(10.))
                .child(primary("report-open", "open it in your browser").child(icon("external", 14., c(theme::CRUST))).on_click(cx.listener(|app, _, _, cx| app.report_open_issue(cx))))
                .child(outline("report-copy", "copy the link").child(icon("copy", 14., c(theme::TEXT))).on_click(cx.listener(|app, _, _, cx| app.report_copy_link(cx))))
                .child(outline("report-done", "done").on_click(cx.listener(|app, _, _, cx| app.report_done(cx)))),
        )
}

fn failed(app: &FrameApp, why: String, cx: &mut Context<FrameApp>) -> Div {
    let can_retry = app.report.preview.is_some();
    div()
        .flex()
        .flex_col()
        .gap(px(14.))
        .child(
            card()
                .flex()
                .items_start()
                .gap(px(14.))
                .p(px(18.))
                .bg(a(theme::RED, 0.06))
                .border_color(a(theme::RED, 0.28))
                .child(div().size(px(40.)).flex_none().flex().items_center().justify_center().rounded(px(12.)).bg(a(theme::RED, 0.12)).child(icon("x", 18., c(theme::RED))))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(div().font_weight(FontWeight::MEDIUM).text_color(c(theme::RED)).child("it didn't send"))
                        .child(div().text_size(px(13.)).text_color(c(theme::SUBTEXT1)).whitespace_normal().child(why))
                        .child(div().text_size(px(12.)).text_color(c(theme::SUBTEXT0)).child("nothing was posted. what you wrote is still here.")),
                ),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(12.))
                .when(can_retry, |d| d.child(primary("report-retry", "try again").on_click(cx.listener(|app, _, _, cx| app.report_send(cx)))))
                .child(outline("report-edit", "edit").on_click(cx.listener(|app, _, window, cx| app.report_edit(window, cx)))),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typing_stops_at_the_relays_limit_and_keeps_new_lines() {
        let mut d = String::new();
        push_typed(&mut d, "it went\nwrong\t");
        assert_eq!(d, "it went\nwrong");
        let mut d = "x".repeat(MOST - 2);
        push_typed(&mut d, "abc");
        assert_eq!(d.chars().count(), MOST);
        assert!(d.ends_with("ab"));
        push_typed(&mut d, "more");
        assert_eq!(d.chars().count(), MOST);
    }

    fn report(headset: Option<serde_json::Value>, unavailable: Option<&str>) -> Report {
        Report {
            version: 1,
            source: "desktop".into(),
            description: "it broke".into(),
            app: AppInfo { name: "framecorder desktop".into(), version: "0.2.0".into(), os: "Ubuntu 24.04".into(), device: "linux pc, x11, appimage".into(), arch: "x86_64".into() },
            headset,
            headset_unavailable: unavailable.map(String::from),
            app_logs: vec![],
        }
    }

    #[test]
    fn the_summary_says_what_came_from_the_headset_or_why_nothing_did() {
        let diag = serde_json::json!({
            "framecorder": {"version": "0.2.0", "channel": "dev"},
            "os": {"name": "SteamOS", "version": "3.7.8", "build_id": "20251101.1"},
            "steam": {"name": "coah", "steamvr_version": "2.11.3", "steamvr_branch": "beta"},
            "recent_errors": ["a", "b", "c"],
            "logs": [{"name": "recorder.log", "text": ""}, {"name": "vrserver.txt", "text": ""}]
        });
        let lines = summary(&report(Some(diag), None));
        let find = |name: &str| lines.iter().find(|l| l.name == name).map(|l| l.value.as_str());
        assert_eq!(find("the headset"), Some("framecorder 0.2.0 (dev) · SteamOS 3.7.8 20251101.1 · steamvr 2.11.3 beta"));
        assert_eq!(find("steam name"), Some("coah"));
        assert_eq!(find("recent errors"), Some("3 lines from the last 48 hours"));
        assert_eq!(find("its logs"), Some("recorder.log, vrserver.txt"));

        let lines = summary(&report(None, Some("no headset is paired")));
        assert_eq!(lines.iter().find(|l| l.name == "the headset").unwrap().value, "not included: no headset is paired");
        assert!(lines.iter().all(|l| l.name != "steam name"));
    }
}

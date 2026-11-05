//! report an issue: say what went wrong, and a github issue opens with it and
//! the diagnostics that go with it. the collecting and sending are the app core's
//! (`framecorder_app_lib::core::report`), this is the screen and its steps.

use framecorder_app_lib::core::report::{self as core, AppInfo, Sent};
use gpui::{
    div, prelude::*, px, rgb_to_hsla, AnyElement, ClipboardItem, Context, Div, FocusHandle, FontWeight, HighlightStyle,
    KeyDownEvent, StyledText, Task, Window,
};

use crate::app::{FrameApp, Page};
use crate::theme::{self, a, c};
use crate::widgets::{card, data, icon, label, outline, primary, spinner, title};

/// the relay takes 1 to 4000 characters
pub const MOST: usize = 4000;
const ISSUES: &str = "github.com/coah80/framecorder";

pub enum Phase {
    Writing,
    /// gathering the frame's and this app's diagnostics, then posting
    Sending,
    Sent(Sent),
    Failed(String),
}

pub struct State {
    pub description: String,
    pub focus: FocusHandle,
    pub phase: Phase,
    task: Option<Task<()>>,
}

impl State {
    pub fn new(focus: FocusHandle) -> Self {
        Self { description: String::new(), focus, phase: Phase::Writing, task: None }
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

impl FrameApp {
    pub fn open_report(&mut self, cx: &mut Context<Self>) {
        if matches!(self.report.phase, Phase::Sent(_)) {
            self.report_reset();
        }
        self.go(Page::Report, cx);
    }

    fn report_reset(&mut self) {
        self.report.description.clear();
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

    pub fn report_can_send(&self) -> bool {
        matches!(self.report.phase, Phase::Writing | Phase::Failed(_)) && !self.report.description.trim().is_empty()
    }

    pub fn report_edit(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.report.phase = Phase::Writing;
        self.report.task = None;
        self.report.focus.focus(window, cx);
        cx.notify();
    }

    /// gathers what goes with the description and posts it
    pub fn report_send(&mut self, cx: &mut Context<Self>) {
        if !self.report_can_send() {
            return;
        }
        self.report.phase = Phase::Sending;
        cx.notify();
        if self.demo {
            self.report.task = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(std::time::Duration::from_millis(1500)).await;
                let _ = this.update(cx, |app, cx| {
                    app.report.phase = Phase::Sent(Sent { number: 128, url: format!("https://{ISSUES}/issues/128") });
                    cx.notify();
                });
            }));
            return;
        }
        let (engine, description) = (self.core.engine.clone(), self.report.description.clone());
        let job = self.core.rt.spawn(async move {
            let report = engine.report_preview(&description, app_info()).await;
            core::send(&report).await.map_err(|e| e.0)
        });
        self.report.task = Some(cx.spawn(async move |this, cx| {
            let res = job.await.unwrap_or_else(|e| Err(format!("couldn't send the report: {e}")));
            let _ = this.update(cx, |app, cx| {
                app.report.phase = match res {
                    Ok(sent) => Phase::Sent(sent),
                    Err(e) => Phase::Failed(e),
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
        Phase::Sending => sending().into_any_element(),
        Phase::Sent(sent) => sent_card(sent, cx).into_any_element(),
        Phase::Failed(why) => failed(why.clone(), cx).into_any_element(),
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
        .child(title("report an issue", 26.))
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
                div().flex().justify_end().child(
                    data(format!("{count} / {MOST}")).text_size(px(11.)).text_color(c(if count >= MOST { theme::PEACH } else { theme::OVERLAY2 })),
                ),
            ),
        )
        .child(div().flex().child(if app.report_can_send() {
            primary("report-send", "send it").on_click(cx.listener(|app, _, _, cx| app.report_send(cx)))
        } else {
            primary("report-send", "send it").opacity(0.35).cursor_default()
        }))
}

/// the description as typed, with a mauve caret on the end while the box has the keyboard
fn typed_text(text: &str, focused: bool) -> StyledText {
    if !focused {
        return StyledText::new(text.to_string());
    }
    let caret = HighlightStyle { color: Some(rgb_to_hsla(c(theme::MAUVE))), ..Default::default() };
    StyledText::new(format!("{text}|")).with_highlights([(text.len()..text.len() + 1, caret)])
}

fn sending() -> Div {
    card()
        .flex()
        .items_center()
        .gap(px(14.))
        .p(px(18.))
        .child(spinner("report-sending", 18., c(theme::MAUVE)))
        .child(div().font_weight(FontWeight::MEDIUM).child("sending"))
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

fn failed(why: String, cx: &mut Context<FrameApp>) -> Div {
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
                        .child(div().text_size(px(13.)).text_color(c(theme::SUBTEXT1)).whitespace_normal().child(why)),
                ),
        )
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(12.))
                .child(primary("report-retry", "try again").on_click(cx.listener(|app, _, _, cx| app.report_send(cx))))
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
}

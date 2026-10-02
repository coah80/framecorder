//! pairing: what to do on the frame on the left, pick it and type its code on the right.

use gpui::{div, prelude::*, px, AnyElement, Context, Div, FontWeight, SharedString, Window};

use crate::app::{FrameApp, Page};
use crate::theme::{self, a, c};
use crate::widgets::{card, data, icon, primary, spinner, title, wordmark};

pub fn render(app: &mut FrameApp, window: &mut Window, cx: &mut Context<FrameApp>) -> impl IntoElement {
    // the code box is the only thing to type in, so it has the keyboard
    if !app.pairing.busy && window.focused(cx).is_none() {
        app.pairing.focus.focus(window, cx);
    }
    let can_cancel = !app.statuses.is_empty() || app.demo;
    div().size_full().flex().child(left()).child(
        div()
            .w(px(460.))
            .flex_none()
            .h_full()
            .relative()
            .flex()
            .flex_col()
            .justify_center()
            .gap(px(28.))
            .px(px(40.))
            .when(can_cancel, |d| {
                d.child(
                    div()
                        .id("cancel")
                        .absolute()
                        .top(px(30.))
                        .right(px(40.))
                        .text_size(px(13.))
                        .text_color(c(theme::MAUVE))
                        .cursor_pointer()
                        .hover(|s| s.text_color(c(theme::MAUVE_LIGHT)))
                        .on_click(cx.listener(|app, _, _, cx| app.go(Page::Clips, cx)))
                        .child("cancel"),
                )
            })
            .child(pick(app, cx))
            .child(code(app, window, cx)),
    )
}

fn step(n: &str) -> Div {
    div()
        .size(px(28.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(9.))
        .bg(a(theme::MAUVE, 0.12))
        .text_color(c(theme::MAUVE))
        .child(data(n.to_string()))
}

fn step_title(text: &str) -> Div {
    div().font_family(theme::HEADING).font_weight(FontWeight::BOLD).text_size(px(15.)).child(text.to_string())
}

fn left() -> impl IntoElement {
    let bullet = |parts: Vec<(&'static str, bool)>| {
        div()
            .flex()
            .items_center()
            .gap(px(12.))
            .child(div().size(px(6.)).flex_none().rounded_full().bg(c(theme::MAUVE)))
            .child(div().flex().gap(px(4.)).children(
                parts.into_iter().map(|(t, strong)| div().when(strong, |d| d.font_weight(FontWeight::MEDIUM)).child(t)),
            ))
    };
    div()
        .flex_1()
        .h_full()
        .flex()
        .flex_col()
        .justify_between()
        .gap(px(32.))
        .px(px(44.))
        .py(px(40.))
        .bg(c(theme::MANTLE))
        .border_r_1()
        .border_color(theme::line())
        .child(wordmark())
        .child(
            div().flex().flex_col().gap(px(14.)).child(title("pair your frame", 38.)).child(
                div()
                    .max_w(px(340.))
                    .text_size(px(15.))
                    .text_color(c(theme::SUBTEXT1))
                    .child("clips and recordings come straight over your wi-fi, nothing goes through the internet."),
            ),
        )
        .child(
            card()
                .flex()
                .flex_col()
                .gap(px(16.))
                .p(px(22.))
                .rounded(px(18.))
                .child(div().flex().items_center().gap(px(12.)).child(step("1")).child(step_title("on your frame")))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .child(bullet(vec![("open the", false), ("framecorder", true), ("tab", false)]))
                        .child(bullet(vec![("go to", false), ("settings,", true), ("then", false), ("sync", true)]))
                        .child(bullet(vec![("hit", false), ("pair a device", true)])),
                )
                .child(
                    div()
                        .pt(px(12.))
                        .border_t_1()
                        .border_color(theme::line())
                        .text_size(px(13.))
                        .text_color(c(theme::SUBTEXT0))
                        .child("it shows a 6 digit code. you'll type it on the right."),
                ),
        )
        .child(
            div()
                .max_w(px(340.))
                .text_size(px(12.))
                .text_color(c(theme::OVERLAY2))
                .child("syncing only happens while the frame is on, on the same wi-fi, and this app is open"),
        )
}

fn pick(app: &FrameApp, cx: &mut Context<FrameApp>) -> impl IntoElement {
    let found: Vec<AnyElement> = app
        .pairing
        .found
        .iter()
        .map(|f| {
            let on = app.pairing.selected.as_deref() == Some(f.fingerprint.as_str());
            let fp = f.fingerprint.clone();
            let ip = f.addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(&f.addr).to_string();
            div()
                .id(SharedString::from(format!("found-{fp}")))
                .flex()
                .items_center()
                .gap(px(12.))
                .h(px(56.))
                .px(px(14.))
                .rounded(px(12.))
                .border_1()
                .cursor_pointer()
                .border_color(if on { c(theme::MAUVE) } else { a(theme::SURFACE1, 0.6) })
                .bg(if on { a(theme::MAUVE, 0.08) } else { theme::card() })
                .when(!on, |d| d.hover(|s| s.border_color(c(theme::SURFACE2))))
                .on_click(cx.listener(move |app, _, _, cx| {
                    app.pairing.selected = Some(fp.clone());
                    app.pairing.error = None;
                    cx.notify();
                }))
                .child(icon("headset", 20., c(if on { theme::MAUVE } else { theme::OVERLAY2 })))
                .child(div().flex_1().truncate().font_weight(FontWeight::MEDIUM).child(f.name.clone()))
                .child(data(ip).text_size(px(12.)).text_color(c(if on { theme::MAUVE } else { theme::OVERLAY2 })))
                .into_any_element()
        })
        .collect();
    let empty = found.is_empty();
    div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(12.))
                .child(step("2"))
                .child(step_title("pick your frame").flex_1())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_size(px(13.))
                        .text_color(c(theme::MAUVE))
                        .child(spinner("looking", 14., c(theme::MAUVE)))
                        .child("looking"),
                ),
        )
        .child(div().flex().flex_col().gap(px(8.)).children(found))
        .when(empty, |d| {
            d.child(
                div()
                    .text_size(px(13.))
                    .text_color(c(theme::SUBTEXT0))
                    .child("looking for frames on this network. make sure the frame's on and on the same wi-fi."),
            )
        })
}

fn code(app: &FrameApp, window: &mut Window, cx: &mut Context<FrameApp>) -> impl IntoElement {
    let p = &app.pairing;
    let focused = p.focus.is_focused(window);
    let digits: Vec<char> = p.code.chars().collect();
    let cells = (0..6).map(|i| {
        let current = focused && i == digits.len();
        div()
            .flex_1()
            .h(px(60.))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(12.))
            .border_1()
            .border_color(if current { c(theme::MAUVE) } else { a(theme::SURFACE1, 0.8) })
            .bg(a(theme::CRUST, 0.4))
            .font_family(theme::DATA)
            .text_size(px(28.))
            .child(match digits.get(i) {
                Some(d) => div().child(d.to_string()),
                None => div().text_color(c(theme::SURFACE2)).child("0"),
            })
    });
    let ready = app.can_pair();
    let busy = p.busy;
    div()
        .flex()
        .flex_col()
        .gap(px(12.))
        .child(div().flex().items_center().gap(px(12.)).child(step("3")).child(step_title("type the code it shows")))
        .child(
            div()
                .id("code")
                .track_focus(&p.focus)
                .on_key_down(cx.listener(|app, ev, window, cx| app.code_key(ev, window, cx)))
                .on_click(cx.listener(|app, _, window, cx| app.pairing.focus.focus(window, cx)))
                .flex()
                .gap(px(8.))
                .cursor_text()
                .children(cells),
        )
        .child(
            primary("pair", if busy { "pairing..." } else { "pair" })
                .h(px(48.))
                .text_size(px(15.))
                .when(!ready, |d| d.opacity(0.35).cursor_default())
                .when(ready, |d| d.on_click(cx.listener(|app, _, _, cx| app.submit_pair(cx)))),
        )
        .when_some(p.error.clone(), |d, e| d.child(div().text_size(px(13.)).text_color(c(theme::RED)).child(e)))
}

//! settings: your frames and this computer.

use gpui::{div, prelude::*, px, AnyElement, Context, Div, FontWeight, SharedString};

use crate::app::{status_color, status_text, FrameApp, Page};
use crate::sidebar::short_path;
use crate::{format, sync};
use crate::theme::{self, a, c};
use crate::widgets::{card, danger, data, destructive, dot, icon, label, outline, switch, title};
use framecorder_app_lib::core::engine::Status;

fn row(first: bool) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(14.))
        .px(px(16.))
        .py(px(14.))
        .when(!first, |d| d.border_t_1().border_color(theme::line()))
}

/// a row's name, on its own
fn name(head: impl Into<SharedString>) -> Div {
    div().flex_1().min_w_0().font_weight(FontWeight::MEDIUM).child(head.into())
}

fn text(head: impl Into<SharedString>, sub: impl IntoElement) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(div().font_weight(FontWeight::MEDIUM).child(head.into()))
        .child(div().text_size(px(12.)).text_color(c(theme::SUBTEXT0)).child(sub))
}

fn section(name: &str, body: impl IntoElement) -> Div {
    div().flex().flex_col().gap(px(10.)).child(label(name)).child(body)
}

/// the question under a frame's row before it goes. it has the keyboard:
/// esc or a click anywhere else keeps the frame, enter unpairs it
fn confirm(app: &FrameApp, s: &Status, cx: &mut Context<FrameApp>) -> impl IntoElement {
    let fp = s.fingerprint.clone();
    div()
        .id(SharedString::from(format!("confirm-{fp}")))
        .track_focus(&app.confirm)
        .on_key_down(cx.listener(|app, ev, window, cx| app.confirm_key(ev, window, cx)))
        .on_mouse_down_out(cx.listener(|app, _, _, cx| app.keep_frame(cx)))
        .flex()
        .items_center()
        .gap(px(14.))
        .px(px(16.))
        .py(px(14.))
        .bg(a(theme::RED, 0.05))
        .border_t_1()
        .border_color(a(theme::RED, 0.22))
        .child(
            div()
                .size(px(36.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(10.))
                .bg(a(theme::RED, 0.12))
                .child(icon("unlink", 18., c(theme::RED))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(div().font_weight(FontWeight::MEDIUM).text_color(c(theme::RED)).child(format!("unpair {}?", s.name)))
                .child(div().text_size(px(12.)).text_color(c(theme::SUBTEXT1)).child(
                    "clips already on this computer stay.",
                )),
        )
        .child(
            outline(SharedString::from(format!("keep-{fp}")), "keep it")
                .on_click(cx.listener(|app, _, _, cx| app.keep_frame(cx))),
        )
        .child(
            destructive(SharedString::from(format!("unpair-now-{fp}")), "unpair")
                .on_click(cx.listener(move |app, _, _, cx| app.unpair(&fp, cx))),
        )
}

pub fn render(app: &mut FrameApp, cx: &mut Context<FrameApp>) -> impl IntoElement {
    let frames: Vec<AnyElement> = app
        .statuses
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let fp = s.fingerprint.clone();
            let ip = s.addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(&s.addr).to_string();
            let mut sub = vec![status_text(s).to_string(), ip];
            if let Some(u) = &s.update {
                sub.push(format!("framecorder {}", u.installed));
            }
            if let Some(t) = sync::last_sync(&app.clips, &s.fingerprint) {
                sub.push(format!("synced {}", format::ago(t)));
            }
            let asking = app.unpairing.as_deref() == Some(s.fingerprint.as_str());
            let line = row(i == 0).child(dot(status_color(s), false)).child(text(s.name.clone(), sub.join(" · "))).when(
                !asking,
                |d| {
                    d.child(
                        danger(SharedString::from(format!("unpair-{fp}")), "unpair")
                            .on_click(cx.listener(move |app, _, window, cx| app.ask_unpair(fp.clone(), window, cx))),
                    )
                },
            );
            if asking {
                div().flex().flex_col().child(line).child(confirm(app, s, cx)).into_any_element()
            } else {
                line.into_any_element()
            }
        })
        .collect();
    let first_other = frames.is_empty();

    div()
        .id("settings-page")
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_y_scroll()
        .px(px(28.))
        .pt(px(26.))
        .pb(px(32.))
        .flex()
        .flex_col()
        .gap(px(22.))
        .child(title("settings", 26.))
        .child(section(
            "frames",
            card().flex().flex_col().children(frames).child(
                row(first_other)
                    .child(name("another frame?"))
                    .child(
                        outline("pair-another", "pair a frame")
                            .child(icon("plus", 16., c(theme::TEXT)))
                            .on_click(cx.listener(|app, _, _, cx| app.go(Page::Pair, cx))),
                    ),
            ),
        ))
        .child(section(
            "this computer",
            card()
                .flex()
                .flex_col()
                .when_some(app.autostart, |d, on| {
                    d.child(
                        row(true)
                            .child(name("start with the computer"))
                            .child(switch("autostart", on).on_click(cx.listener(move |app, _, _, cx| app.set_autostart(!on, cx)))),
                    )
                })
                .when(app.tray.is_some() || app.demo, |d| {
                    let on = app.background;
                    d.child(
                        row(app.autostart.is_none())
                            .child(name("keep running when closed"))
                            .child(switch("background", on).on_click(cx.listener(move |app, _, _, cx| app.set_background(!on, cx)))),
                    )
                })
                .child(
                    row(app.autostart.is_none() && app.tray.is_none() && !app.demo)
                        .child(name("say when a clip lands"))
                        .child(switch("notify", app.notify).on_click(cx.listener(|app, _, _, cx| app.set_notify(!app.notify, cx)))),
                )
                .child(
                    row(false)
                        .child(text("anonymous stats", "once a day: this app's version, system, how it's set up and how many files it synced. nothing about you or your videos"))
                        .child(switch("stats", app.stats).on_click(cx.listener(|app, _, _, cx| app.set_stats(!app.stats, cx)))),
                )
                .child(
                    row(false)
                        .child(text("clips are saved to", data(short_path(&app.core.download_dir))))
                        .child(outline("open", "open").on_click(cx.listener(|app, _, _, cx| app.open_folder(cx)))),
                )
                .child(
                    row(false)
                        .child(name("quit framecorder"))
                        .child(outline("quit", "quit").on_click(cx.listener(|app, _, _, cx| app.quit(cx)))),
                )
                .child(row(false).child(name(format!("framecorder {}", env!("CARGO_PKG_VERSION"))))),
        ))
}

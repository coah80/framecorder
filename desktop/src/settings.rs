//! settings: your frames, this computer, and how syncing works.

use gpui::{div, prelude::*, px, AnyElement, Context, Div, FontWeight, SharedString};

use crate::app::{quit, status_color, status_text, FrameApp, Page};
use crate::sidebar::short_path;
use crate::theme::{self, c};
use crate::widgets::{card, danger, data, dot, icon, label, outline, switch, title};

fn row(first: bool) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(14.))
        .px(px(16.))
        .py(px(14.))
        .when(!first, |d| d.border_t_1().border_color(theme::line()))
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
            row(i == 0)
                .child(dot(status_color(s), false))
                .child(text(s.name.clone(), sub.join(" · ")))
                .child(
                    danger(SharedString::from(format!("forget-{fp}")), "forget")
                        .on_click(cx.listener(move |app, _, _, cx| app.forget(&fp, cx))),
                )
                .into_any_element()
        })
        .collect();
    let first_other = frames.is_empty();

    div()
        .id("settings")
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
                    .child(text("another frame?", "you can pair more than one"))
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
                            .child(text("start with the computer", "opens when you log in, so clips arrive without you opening it"))
                            .child(switch("autostart", on).on_click(cx.listener(move |app, _, _, cx| app.set_autostart(!on, cx)))),
                    )
                })
                .when(app.tray.is_some() || app.demo, |d| {
                    let on = app.background;
                    let what = if on {
                        if cfg!(target_os = "macos") { "closing the window leaves it syncing in the menu bar" } else { "closing the window leaves it syncing in the tray" }
                    } else {
                        "closing the window quits it. clips catch up the next time it's open"
                    };
                    d.child(
                        row(app.autostart.is_none())
                            .child(text("keep running when closed", what))
                            .child(switch("background", on).on_click(cx.listener(move |app, _, _, cx| app.set_background(!on, cx)))),
                    )
                })
                .child(
                    row(app.autostart.is_none() && app.tray.is_none() && !app.demo)
                        .child(text("clips are saved to", data(short_path(&app.core.download_dir))))
                        .child(outline("open", "open").on_click(cx.listener(|app, _, _, cx| app.open_folder(cx)))),
                )
                .child(
                    row(false)
                        .child(text(
                            "quit framecorder",
                            if app.background && (app.tray.is_some() || app.demo) {
                                "stops syncing until you open it again, clips catch up then"
                            } else {
                                "stops syncing until you open it again, clips catch up then. closing the window does this too"
                            },
                        ))
                        .child(outline("quit", "quit").on_click(|_, _, cx| quit(cx))),
                ),
        ))
        .child(section(
            "how syncing works",
            div().grid().grid_cols(3).gap(px(10.)).children(
                [
                    ("bolt", "a clip is sent the moment it's saved on your frame"),
                    ("wifi", "only while the frame is on, on the same wi-fi, and this app is open"),
                    ("lock", "nothing goes through the internet, straight from the frame to here"),
                ]
                .map(|(i, t)| {
                    card()
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .p(px(14.))
                        .child(icon(i, 18., c(theme::MAUVE)))
                        .child(div().text_size(px(13.)).text_color(c(theme::SUBTEXT1)).child(t))
                }),
            ),
        ))
}

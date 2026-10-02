//! the library: everything from the frame, a day at a time, as a grid or a list.

use gpui::{div, img, prelude::*, px, relative, AnyElement, Context, Div, FontWeight, ObjectFit, SharedString};

use crate::app::{Filter, FrameApp, Page, Thumb};
use crate::format;
use crate::sync::Clip;
use crate::theme::{self, a, c};
use crate::widgets::{card, data, icon, kind_chip, label, primary, spinner, title};
use framecorder_app_lib::core::engine::{State, Status};

pub fn render(app: &mut FrameApp, cx: &mut Context<FrameApp>) -> impl IntoElement {
    let problems: Vec<AnyElement> =
        app.problems().into_iter().cloned().collect::<Vec<_>>().iter().map(|s| problem(s, cx)).collect();
    div().flex_1().min_w_0().h_full().flex().flex_col().child(header(app, cx)).child(
        div()
            .id("gallery")
            .flex_1()
            .min_h(px(0.))
            .overflow_y_scroll()
            .px(px(28.))
            .pt(px(4.))
            .pb(px(28.))
            .flex()
            .flex_col()
            .gap(px(26.))
            .children(problems)
            .children(days(app, cx)),
    )
}

fn header(app: &FrameApp, cx: &mut Context<FrameApp>) -> impl IntoElement {
    let clips = app.clips.iter().filter(|c| c.is_clip).count();
    let counts = [
        (Filter::All, "all", app.clips.len()),
        (Filter::Clip, "clips", clips),
        (Filter::Recording, "recordings", app.clips.len() - clips),
    ];
    div()
        .flex()
        .items_end()
        .justify_between()
        .gap(px(16.))
        .px(px(28.))
        .pt(px(26.))
        .pb(px(18.))
        .child(div().flex().flex_col().gap(px(4.)).child(title("clips", 26.)).child(
            div().text_size(px(13.)).text_color(c(theme::SUBTEXT0)).child("everything from your frame, newest first"),
        ))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(seg().children(counts.into_iter().map(|(f, name, n)| {
                    let on = app.filter == f;
                    div()
                        .id(name)
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .h(px(34.))
                        .px(px(12.))
                        .rounded(px(9.))
                        .text_size(px(13.))
                        .font_weight(FontWeight::MEDIUM)
                        .cursor_pointer()
                        .bg(if on { c(theme::MAUVE) } else { a(0, 0.) })
                        .text_color(c(if on { theme::CRUST } else { theme::SUBTEXT0 }))
                        .when(!on, |d| d.hover(|s| s.text_color(c(theme::MAUVE))))
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.filter = f;
                            cx.notify();
                        }))
                        .child(name)
                        .when(n > 0, |d| d.child(data(n.to_string()).opacity(0.7)))
                })))
                .child(
                    seg()
                        .child(layout_button("grid", app.grid, true, cx))
                        .child(layout_button("list", !app.grid, false, cx)),
                ),
        )
}

fn seg() -> Div {
    div().flex().gap(px(2.)).p(px(3.)).rounded(px(12.)).bg(theme::card()).border_1().border_color(theme::card_line())
}

fn layout_button(name: &'static str, on: bool, grid: bool, cx: &mut Context<FrameApp>) -> impl IntoElement {
    let fg = if on { theme::TEXT } else { theme::OVERLAY2 };
    div()
        .id(name)
        .size(px(34.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(9.))
        .cursor_pointer()
        .when(on, |d| d.bg(c(theme::SURFACE1)))
        .on_click(cx.listener(move |app, _, _, cx| {
            app.grid = grid;
            cx.notify();
        }))
        .child(icon(name, 16., c(fg)))
}

/// a frame that can't sync gets a whole panel saying why, and what to do
fn problem(s: &Status, cx: &mut Context<FrameApp>) -> AnyElement {
    let red = matches!(s.state, State::Unreachable | State::Full);
    let tint = if red { theme::RED } else { theme::PEACH };
    let (icon_name, heading, text, action): (&str, String, String, &str) = match s.state {
        State::Unreachable => (
            "wifi-off",
            format!("can't reach {}", s.name),
            "clips wait on the frame until it's back, nothing is lost. check that".into(),
            "try again",
        ),
        State::Full => (
            "disk",
            "this computer is out of space".into(),
            format!(
                "the clips are still on {}, nothing is lost. free up some space and they sync on their own.",
                s.name
            ),
            "try again",
        ),
        State::Unpaired => (
            "headset",
            format!("{} forgot this device", s.name),
            "it was removed on the frame. pair again to keep syncing.".into(),
            "pair again",
        ),
        _ => (
            "lock",
            format!("that isn't {}", s.name),
            "something else answered where your frame was, so we're not talking to it.".into(),
            "pair again",
        ),
    };
    let pair = action == "pair again";
    div()
        .flex()
        .items_start()
        .gap(px(18.))
        .p(px(20.))
        .rounded(px(16.))
        .bg(a(tint, 0.05))
        .border_1()
        .border_color(a(tint, 0.22))
        .child(
            div()
                .size(px(40.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(12.))
                .bg(a(tint, 0.12))
                .child(icon(icon_name, 20., c(tint))),
        )
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .gap(px(10.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .font_family(theme::HEADING)
                                .font_weight(FontWeight::BOLD)
                                .text_size(px(17.))
                                .text_color(c(tint))
                                .child(heading),
                        )
                        .child(div().text_color(c(theme::SUBTEXT1)).child(text)),
                )
                .when(s.state == State::Unreachable, |d| {
                    d.child(
                        div().grid().grid_cols(3).gap(px(8.)).children(
                            [
                                ("power", "your frame is on"),
                                ("wifi", "it's on this wi-fi"),
                                ("clips", "framecorder is running on it"),
                            ]
                            .map(|(i, t)| {
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(6.))
                                    .p(px(12.))
                                    .rounded(px(12.))
                                    .bg(a(theme::CRUST, 0.35))
                                    .child(icon(i, 18., c(theme::SUBTEXT1)))
                                    .child(div().text_size(px(13.)).child(t))
                            }),
                        ),
                    )
                })
                .when_some(s.message.clone(), |d, m| {
                    d.child(data(m).text_size(px(12.)).text_color(c(theme::OVERLAY2)))
                }),
        )
        .child(primary(SharedString::from(format!("fix-{}", s.fingerprint)), action).flex_none().h(px(44.)).on_click(
            cx.listener(move |app, _, _, cx| {
                if pair {
                    app.go(Page::Pair, cx);
                } else {
                    app.retry(cx);
                }
            }),
        ))
        .into_any_element()
}

fn days(app: &FrameApp, cx: &mut Context<FrameApp>) -> Vec<AnyElement> {
    let shown: Vec<&Clip> = app
        .clips
        .iter()
        .filter(|c| match app.filter {
            Filter::All => true,
            Filter::Clip => c.is_clip,
            Filter::Recording => !c.is_clip,
        })
        .collect();

    let incoming = app.progress.is_some();
    if shown.is_empty() && !incoming {
        let text = if app.clips.is_empty() {
            "nothing yet. save a clip or a recording on your frame and it shows up here."
        } else if app.filter == Filter::Clip {
            "no clips yet, just recordings."
        } else {
            "no recordings yet, just clips."
        };
        return vec![div()
            .py(px(56.))
            .px(px(20.))
            .flex()
            .justify_center()
            .rounded(px(16.))
            .border_1()
            .border_dashed()
            .border_color(theme::card_line())
            .text_color(c(theme::SUBTEXT0))
            .child(text)
            .into_any_element()];
    }

    let today = chrono::Local::now().date_naive();
    let mut groups: Vec<(chrono::NaiveDate, Vec<&Clip>)> = Vec::new();
    for clip in shown {
        let key = format::day_key(clip.created);
        match groups.last_mut() {
            Some((d, list)) if *d == key => list.push(clip),
            _ => groups.push((key, vec![clip])),
        }
    }
    if incoming && groups.first().is_none_or(|(d, _)| *d != today) {
        groups.insert(0, (today, Vec::new()));
    }

    let mut index = 0usize;
    groups
        .into_iter()
        .map(|(day, list)| {
            let is_today = day == today;
            let name = list.first().map(|c| format::day(c.created)).unwrap_or_else(|| "today".into());
            let mut items: Vec<AnyElement> = Vec::new();
            if incoming && is_today {
                items.push(if app.grid { incoming_tile(app) } else { incoming_row(app) });
            }
            for clip in list {
                index += 1;
                items.push(if app.grid { tile(app, clip, index, cx) } else { row(app, clip, index, cx) });
            }
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .child(label(&name))
                        .when(incoming && is_today, |d| d.child(counter(app))),
                )
                .child(if app.grid {
                    div().grid().grid_cols(3).gap_x(px(16.)).gap_y(px(18.)).children(items)
                } else {
                    card().overflow_hidden().flex().flex_col().children(items)
                })
                .into_any_element()
        })
        .collect()
}

/// "getting 1 of 3 · 2 more coming", next to today
fn counter(app: &FrameApp) -> impl IntoElement {
    let queued = app.progress.as_ref().map(|p| p.queued).unwrap_or(0);
    let text = match &app.batch {
        Some(b) if b.total > 1 => {
            let n = (b.done + 1).min(b.total);
            if queued > 0 {
                format!("getting {n} of {} · {queued} more coming", b.total)
            } else {
                format!("getting {n} of {}", b.total)
            }
        }
        _ => "getting a new one".into(),
    };
    div()
        .flex()
        .items_center()
        .gap(px(6.))
        .px(px(10.))
        .py(px(2.))
        .rounded_full()
        .bg(a(theme::MAUVE, 0.12))
        .text_color(c(theme::MAUVE))
        .text_size(px(12.))
        .font_weight(FontWeight::MEDIUM)
        .child(spinner("counter-spin", 12., c(theme::MAUVE)))
        .child(text)
}

fn pct(app: &FrameApp) -> f32 {
    app.progress.as_ref().map(|p| if p.total > 0 { p.done as f32 / p.total as f32 } else { 1.0 }).unwrap_or(0.0)
}

fn incoming_tile(app: &FrameApp) -> AnyElement {
    let p = pct(app);
    let total = app.progress.as_ref().map(|p| p.total).unwrap_or(0);
    div()
        .flex()
        .flex_col()
        .gap(px(8.))
        .child(
            div()
                .relative()
                .w_full()
                .aspect_ratio(16. / 9.)
                .rounded(px(12.))
                .border_1()
                .border_dashed()
                .border_color(a(theme::MAUVE, 0.45))
                .bg(a(theme::MAUVE, 0.05))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .child(icon("download", 22., c(theme::MAUVE)))
                .child(data(format!("{}%", (p * 100.) as u32)).text_size(px(12.)).text_color(c(theme::MAUVE)))
                .child(
                    div()
                        .absolute()
                        .left(px(10.))
                        .right(px(10.))
                        .bottom(px(10.))
                        .h(px(3.))
                        .rounded_full()
                        .overflow_hidden()
                        .bg(a(theme::MAUVE, 0.15))
                        .child(div().h_full().w(relative(p)).bg(c(theme::MAUVE))),
                ),
        )
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(2.))
                .child(div().flex_1().font_weight(FontWeight::MEDIUM).child("incoming"))
                .child(data(format::size(total)).text_size(px(12.)).text_color(c(theme::OVERLAY2))),
        )
        .into_any_element()
}

fn incoming_row(app: &FrameApp) -> AnyElement {
    let p = pct(app);
    div()
        .flex()
        .items_center()
        .gap(px(14.))
        .px(px(10.))
        .py(px(8.))
        .child(
            div()
                .w(px(88.))
                .h(px(50.))
                .flex_none()
                .rounded(px(8.))
                .border_1()
                .border_dashed()
                .border_color(a(theme::MAUVE, 0.45))
                .flex()
                .items_center()
                .justify_center()
                .child(icon("download", 16., c(theme::MAUVE))),
        )
        .child(div().font_weight(FontWeight::MEDIUM).child("incoming"))
        .child(
            div()
                .flex_1()
                .h(px(4.))
                .rounded_full()
                .overflow_hidden()
                .bg(a(theme::MAUVE, 0.15))
                .child(div().h_full().w(relative(p)).bg(c(theme::MAUVE))),
        )
        .child(data(format!("{}%", (p * 100.) as u32)).w(px(48.)).text_size(px(12.)).text_color(c(theme::MAUVE)))
        .into_any_element()
}

fn thumb(app: &FrameApp, clip: &Clip, index: usize) -> Div {
    let tone = theme::TONES[index % theme::TONES.len()];
    let base = div().relative().overflow_hidden().bg(c(tone)).flex().items_center().justify_center();
    match app.thumbs.get(&clip.key) {
        Some(Thumb::Ready(path)) => {
            base.child(img(path.clone()).absolute().inset_0().size_full().object_fit(ObjectFit::Cover))
        }
        _ => base.child(icon("play", 22., a(theme::TEXT, 0.35))),
    }
}

fn size_text(clip: &Clip) -> String {
    if clip.exists {
        format::size(clip.size)
    } else {
        "moved or deleted".into()
    }
}

fn tile(app: &FrameApp, clip: &Clip, index: usize, cx: &mut Context<FrameApp>) -> AnyElement {
    let group = SharedString::from(format!("tile-{index}"));
    let (open, reveal) = (clip.clone(), clip.clone());
    div()
        .relative()
        .group(group.clone())
        .when(!clip.exists, |d| d.opacity(0.45))
        .child(
            div()
                .id(SharedString::from(format!("open-{}", clip.key)))
                .flex()
                .flex_col()
                .gap(px(8.))
                .when(clip.exists, |d| {
                    d.cursor_pointer().on_click(cx.listener(move |app, _, _, cx| app.open_clip(&open, cx)))
                })
                .child(
                    thumb(app, clip, index)
                        .w_full()
                        .aspect_ratio(16. / 9.)
                        .rounded(px(12.))
                        .border_2()
                        .border_color(a(theme::MAUVE, 0.))
                        .when(clip.exists, |d| d.group_hover(group.clone(), |s| s.border_color(c(theme::MAUVE))))
                        .when_some(clip.duration_s, |d, len| {
                            d.child(
                                data(format::length(len))
                                    .absolute()
                                    .right(px(6.))
                                    .bottom(px(6.))
                                    .px(px(7.))
                                    .rounded(px(6.))
                                    .text_size(px(11.))
                                    .bg(a(theme::CRUST, 0.8)),
                            )
                        }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(2.))
                        .child(div().flex_1().font_weight(FontWeight::MEDIUM).child(format::time(clip.created)))
                        .child(kind_chip(clip.is_clip))
                        .child(data(size_text(clip)).text_size(px(12.)).text_color(c(theme::OVERLAY2))),
                ),
        )
        .when(clip.exists, |d| {
            d.child(
                div()
                    .id(SharedString::from(format!("reveal-{}", clip.key)))
                    .absolute()
                    .top(px(6.))
                    .right(px(6.))
                    .size(px(32.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(9.))
                    .bg(a(theme::CRUST, 0.8))
                    .cursor_pointer()
                    .opacity(0.)
                    .group_hover(group, |s| s.opacity(1.))
                    .on_click(cx.listener(move |app, _, _, cx| {
                        cx.stop_propagation();
                        app.reveal_clip(&reveal, cx);
                    }))
                    .child(icon("folder", 16., c(theme::TEXT))),
            )
        })
        .into_any_element()
}

fn row(app: &FrameApp, clip: &Clip, index: usize, cx: &mut Context<FrameApp>) -> AnyElement {
    let (open, reveal) = (clip.clone(), clip.clone());
    div()
        .id(SharedString::from(format!("row-{}", clip.key)))
        .flex()
        .items_center()
        .gap(px(14.))
        .px(px(10.))
        .py(px(8.))
        .when(index > 1, |d| d.border_t_1().border_color(theme::line()))
        .when(!clip.exists, |d| d.opacity(0.45))
        .when(clip.exists, |d| {
            d.cursor_pointer()
                .hover(|s| s.bg(a(theme::MAUVE, 0.08)))
                .on_click(cx.listener(move |app, _, _, cx| app.open_clip(&open, cx)))
        })
        .child(thumb(app, clip, index).w(px(88.)).h(px(50.)).flex_none().rounded(px(8.)))
        .child(div().w(px(76.)).font_weight(FontWeight::MEDIUM).child(format::time(clip.created)))
        .child(kind_chip(clip.is_clip))
        .child(div().flex_1())
        .when_some(clip.duration_s, |d, len| {
            d.child(data(format::length(len)).text_size(px(12.)).text_color(c(theme::SUBTEXT0)))
        })
        .child(
            data(size_text(clip)).min_w(px(60.)).flex().justify_end().text_size(px(12.)).text_color(c(theme::OVERLAY2)),
        )
        .when(clip.exists, |d| {
            d.child(
                div()
                    .id(SharedString::from(format!("rowreveal-{}", clip.key)))
                    .size(px(36.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(9.))
                    .cursor_pointer()
                    .hover(|s| s.bg(c(theme::SURFACE1)))
                    .on_click(cx.listener(move |app, _, _, cx| {
                        cx.stop_propagation();
                        app.reveal_clip(&reveal, cx);
                    }))
                    .child(icon("folder", 16., c(theme::SUBTEXT0))),
            )
        })
        .into_any_element()
}

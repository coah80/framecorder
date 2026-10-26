//! the library: everything from the frame, a day at a time, as a grid or a list.

use gpui::{
    div, img, prelude::*, px, relative, AnyElement, ClickEvent, Context, Div, FontWeight, MouseButton, MouseUpEvent,
    ObjectFit, SharedString, Stateful, Window,
};

use crate::app::{narrow, Filter, FrameApp, Thumb};
use crate::sync::Clip;
use crate::theme::{self, a, c};
use crate::widgets::{card, data, icon, kind_chip, label, outline, primary, spinner, title};
use crate::{format, search, selection};
use framecorder_app_lib::core::engine::{State, Status};

pub fn render(app: &mut FrameApp, window: &mut Window, cx: &mut Context<FrameApp>) -> impl IntoElement {
    // the page holds the keyboard when nothing else does, so ctrl+f works right away
    if window.focused(cx).is_none() {
        app.library.focus(window, cx);
    }
    let searching = app.search.is_focused(window);
    let narrow = narrow(window);
    let today = chrono::Local::now().date_naive();
    let found: Vec<&Clip> = app.clips.iter().filter(|c| search::hit(c, &app.query, today)).collect();
    let shown: Vec<&Clip> = found.iter().copied().filter(|c| kind_ok(app.filter, c)).collect();
    let selected: Vec<Clip> = shown.iter().filter(|c| app.selection.picked.contains(&c.key)).map(|c| (*c).clone()).collect();
    let selecting = !selected.is_empty();
    let problems: Vec<AnyElement> =
        app.problems().into_iter().cloned().collect::<Vec<_>>().iter().map(|s| problem(s, narrow, cx)).collect();
    // the tiles note where they land as they're painted, see selection.rs
    app.selection.tiles.borrow_mut().clear();
    div()
        .id("library")
        .track_focus(&app.library)
        .on_key_down(cx.listener(|app, ev, window, cx| app.library_key(ev, window, cx)))
        .relative()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .flex_col()
        .child(header(app, &found, searching, cx))
        .child(
            div()
                .id("gallery")
                .track_scroll(&app.gallery)
                .on_mouse_down(MouseButton::Left, cx.listener(|app, ev, window, cx| app.press(ev, window, cx)))
                .on_mouse_move(cx.listener(|app, ev, _, cx| app.drag_moved(ev, cx)))
                // in the capture phase, so it runs before a tile button's click can stop the event
                .capture_any_mouse_up(cx.listener(|app, ev: &MouseUpEvent, _, cx| {
                    if ev.button == MouseButton::Left {
                        app.release(cx);
                    }
                }))
                .on_mouse_up_out(MouseButton::Left, cx.listener(|app, _, _, cx| app.release(cx)))
                .flex_1()
                .min_h(px(0.))
                .overflow_y_scroll()
                .px(px(28.))
                .pt(px(4.))
                // room to scroll the last row out from under the bar
                .pb(px(if selecting { 96. } else { 28. }))
                .flex()
                .flex_col()
                .gap(px(26.))
                .children(problems)
                .children(days(app, &shown, selecting, narrow, today, cx)),
        )
        .when(selecting, |d| {
            let all = selected.len() == shown.len();
            d.child(
                div()
                    .absolute()
                    .bottom(px(16.))
                    .left(px(28.))
                    .right(px(28.))
                    .child(selection::bar(app, &selected, all, cx)),
            )
        })
}

fn kind_ok(filter: Filter, clip: &Clip) -> bool {
    match filter {
        Filter::All => true,
        Filter::Clip => clip.is_clip,
        Filter::Recording => !clip.is_clip,
    }
}

/// the clips the library shows, in its order: what the search found, through the filter
pub fn shown(app: &FrameApp, today: chrono::NaiveDate) -> Vec<&Clip> {
    app.clips.iter().filter(|c| search::hit(c, &app.query, today) && kind_ok(app.filter, c)).collect()
}

fn header(app: &FrameApp, found: &[&Clip], searching: bool, cx: &mut Context<FrameApp>) -> impl IntoElement {
    let clips = found.iter().filter(|c| c.is_clip).count();
    let counts = [
        (Filter::All, "all", found.len()),
        (Filter::Clip, "clips", clips),
        (Filter::Recording, "recordings", found.len() - clips),
    ];
    // the filters drop under the search box once the window's too narrow for both
    div()
        .flex()
        .flex_wrap()
        .items_end()
        .justify_between()
        .gap(px(16.))
        .px(px(28.))
        .pt(px(26.))
        .pb(px(18.))
        .child(div().flex().flex_col().gap(px(10.)).child(title("clips", 26.)).child(if app.clips.is_empty() {
            div()
                .text_size(px(13.))
                .text_color(c(theme::SUBTEXT0))
                .child("everything from your frame, newest first")
                .into_any_element()
        } else {
            search_box(app, searching, cx).into_any_element()
        }))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(seg().children(counts.into_iter().map(|(f, name, n)| {
                    let on = app.filter == f;
                    div()
                        .id(SharedString::from(format!("filter-{name}")))
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
                        .on_click(cx.listener(move |app, _, _, cx| app.set_filter(f, cx)))
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

/// the search box, under the title. typed into directly, so it draws its own
/// caret, and the shortcut sits in it until it's used
fn search_box(app: &FrameApp, focused: bool, cx: &mut Context<FrameApp>) -> impl IntoElement {
    let empty = app.query.is_empty();
    let lit = focused || !empty;
    div()
        .id("search")
        .track_focus(&app.search)
        .on_key_down(cx.listener(|app, ev, window, cx| app.search_key(ev, window, cx)))
        .flex()
        .items_center()
        .gap(px(9.))
        .h(px(40.))
        .w(px(300.))
        .px(px(12.))
        .rounded(px(12.))
        .bg(theme::card())
        .border_1()
        .border_color(if focused { c(theme::MAUVE) } else { theme::card_line() })
        .cursor_text()
        .child(icon("search", 15., c(if lit { theme::MAUVE } else { theme::OVERLAY2 })))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .text_size(px(13.))
                .child(if empty {
                    div().truncate().text_color(c(theme::OVERLAY2)).child("search by time, day or name")
                } else {
                    div().truncate().child(app.query.clone())
                })
                .when(focused, |d| d.child(div().flex_none().w(px(1.5)).h(px(16.)).ml(px(1.)).bg(c(theme::MAUVE)))),
        )
        .when(!empty, |d| {
            d.child(
                div()
                    .id("search-clear")
                    .size(px(22.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(6.))
                    .cursor_pointer()
                    .hover(|s| s.bg(c(theme::SURFACE1)))
                    .on_click(cx.listener(|app, _, _, cx| app.clear_search(cx)))
                    .child(icon("x", 12., c(theme::SUBTEXT0))),
            )
        })
        .when(empty && !focused, |d| {
            d.child(
                data(if cfg!(target_os = "macos") { "cmd f" } else { "ctrl f" })
                    .px(px(6.))
                    .py(px(1.))
                    .rounded(px(5.))
                    .border_1()
                    .border_color(theme::card_line())
                    .text_size(px(11.))
                    .text_color(c(theme::OVERLAY2)),
            )
        })
}

fn layout_button(name: &'static str, on: bool, grid: bool, cx: &mut Context<FrameApp>) -> impl IntoElement {
    let fg = if on { theme::TEXT } else { theme::OVERLAY2 };
    div()
        .id(SharedString::from(format!("layout-{name}")))
        .size(px(34.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(9.))
        .cursor_pointer()
        .when(on, |d| d.bg(c(theme::SURFACE1)))
        .on_click(cx.listener(move |app, _, _, cx| app.set_grid(grid, cx)))
        .child(icon(name, 16., c(fg)))
}

/// a frame that can't sync gets a whole panel saying why, and what to do
fn problem(s: &Status, narrow: bool, cx: &mut Context<FrameApp>) -> AnyElement {
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
    let fp = s.fingerprint.clone();
    // the button goes under the words once the window's too narrow for it beside them
    div()
        .flex()
        .flex_wrap()
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
                .min_w(px(240.))
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
                        div().grid().grid_cols(if narrow { 1 } else { 3 }).gap(px(8.)).children(
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
                    app.open_pair_for(fp.clone(), cx);
                } else {
                    app.retry(cx);
                }
            }),
        ))
        .into_any_element()
}

fn days(
    app: &FrameApp,
    shown: &[&Clip],
    selecting: bool,
    narrow: bool,
    today: chrono::NaiveDate,
    cx: &mut Context<FrameApp>,
) -> Vec<AnyElement> {
    // a search shows what it found and nothing else, the download included
    let query = app.query.trim();
    let incoming = app.progress.is_some() && query.is_empty();
    if shown.is_empty() && !incoming {
        let text = if app.clips.is_empty() {
            "nothing yet. save a clip or a recording on your frame and it shows up here.".to_string()
        } else if !query.is_empty() {
            match app.filter {
                Filter::All => format!("nothing matches \"{query}\"."),
                Filter::Clip => format!("no clips match \"{query}\"."),
                Filter::Recording => format!("no recordings match \"{query}\"."),
            }
        } else if app.filter == Filter::Clip {
            "no clips yet, just recordings.".to_string()
        } else {
            "no recordings yet, just clips.".to_string()
        };
        return vec![div()
            .py(px(56.))
            .px(px(20.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(16.))
            .rounded(px(16.))
            .border_1()
            .border_dashed()
            .border_color(theme::card_line())
            .text_color(c(theme::SUBTEXT0))
            .child(text)
            .when(!query.is_empty(), |d| {
                d.child(
                    outline("clear-search", "clear search").on_click(cx.listener(|app, _, _, cx| app.clear_search(cx))),
                )
            })
            .into_any_element()];
    }

    let mut groups: Vec<(chrono::NaiveDate, Vec<&Clip>)> = Vec::new();
    for &clip in shown {
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
                let look = Look { selecting, selected: app.selection.picked.contains(&clip.key) };
                items.push(if app.grid { tile(app, clip, index, look, cx) } else { row(app, clip, index, look, cx) });
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
                    // two across in a narrow window, so the captions still fit under the tiles
                    div().grid().grid_cols(if narrow { 2 } else { 3 }).gap_x(px(16.)).gap_y(px(18.)).children(items)
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

/// "31 MB of 48 MB · 4.2 MB/s", once there's a rate to say
fn so_far(app: &FrameApp) -> String {
    let (done, total) = app.progress.as_ref().map(|p| (p.done, p.total)).unwrap_or((0, 0));
    let mut text = format!("{} of {}", format::size(done), format::size(total));
    if let Some(s) = app.speed.as_ref().filter(|s| s.bytes_per_s > 0.) {
        text.push_str(&format!(" · {}", format::rate(s.bytes_per_s)));
    }
    text
}

fn incoming_tile(app: &FrameApp) -> AnyElement {
    let p = pct(app);
    let (done, total) = app.progress.as_ref().map(|p| (p.done, p.total)).unwrap_or((0, 0));
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
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(5.))
                        .text_size(px(12.))
                        .child(data(format!("{}%", (p * 100.) as u32)).text_color(c(theme::MAUVE)))
                        .when_some(app.speed.as_ref().filter(|s| s.bytes_per_s > 0.), |d, s| {
                            d.child(data(format!("· {}", format::rate(s.bytes_per_s))).text_color(c(theme::OVERLAY2)))
                        }),
                )
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
                .child(div().flex_none().font_weight(FontWeight::MEDIUM).child("incoming"))
                .child(
                    data(format!("{} of {}", format::size(done), format::size(total)))
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(c(theme::OVERLAY2)),
                ),
        )
        .into_any_element()
}

fn incoming_row(app: &FrameApp) -> AnyElement {
    let p = pct(app);
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(10.))
        .py(px(8.))
        .child(
            div()
                .w(px(80.))
                .h(px(45.))
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
        .child(data(so_far(app)).text_size(px(12.)).text_color(c(theme::OVERLAY2)))
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

/// a small square button over a tile, shown while the mouse is on it
fn tile_button(id: String, group: SharedString, icon_name: &str, tint: u32) -> Stateful<Div> {
    div()
        .id(SharedString::from(id))
        .size(px(32.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(9.))
        .bg(a(theme::CRUST, 0.8))
        .cursor_pointer()
        .opacity(0.)
        .group_hover(group, |s| s.opacity(1.))
        .hover(|s| s.bg(c(theme::SURFACE0)))
        .child(icon(icon_name, 16., c(tint)))
}

/// a small button at the end of a row
fn row_button(id: String, icon_name: &str, tint: u32) -> Stateful<Div> {
    div()
        .id(SharedString::from(id))
        .size(px(32.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(px(9.))
        .cursor_pointer()
        .hover(|s| s.bg(c(theme::SURFACE1)))
        .child(icon(icon_name, 16., c(tint)))
}

/// how a tile or row shows its place in the selection: whether anything's
/// picked (the marks come out), and whether this one is
#[derive(Clone, Copy)]
struct Look {
    selecting: bool,
    selected: bool,
}

fn tile(app: &FrameApp, clip: &Clip, index: usize, look: Look, cx: &mut Context<FrameApp>) -> AnyElement {
    let group = SharedString::from(format!("tile-{index}"));
    let (open, reveal, delete) = (clip.clone(), clip.clone(), clip.clone());
    // a picked tile shrinks into its frame, like the phone's
    let inset = if look.selected { 6. } else { 0. };
    div()
        .relative()
        .group(group.clone())
        .child(selection::tile_bounds(&app.selection.tiles, clip.key.clone()))
        .child(
            div()
                .id(SharedString::from(format!("open-{}", clip.key)))
                .flex()
                .flex_col()
                .gap(px(8.))
                .cursor_pointer()
                .on_click(cx.listener(move |app, ev: &ClickEvent, _, cx| app.click_clip(&open, ev.modifiers(), cx)))
                .child(
                    div()
                        .relative()
                        .w_full()
                        .aspect_ratio(16. / 9.)
                        .rounded(px(12.))
                        .border_2()
                        .border_color(a(theme::MAUVE, if look.selected { 1. } else { 0. }))
                        .when(look.selected, |d| d.bg(a(theme::MAUVE, 0.14)))
                        .group_hover(group.clone(), |s| s.border_color(c(theme::MAUVE)))
                        .child(
                            thumb(app, clip, index)
                                .absolute()
                                .inset(px(inset))
                                .rounded(px(if look.selected { 7. } else { 10. })),
                        )
                        .when_some(clip.duration_s, |d, len| {
                            d.child(
                                data(format::length(len))
                                    .absolute()
                                    .right(px(6. + inset))
                                    .bottom(px(6. + inset))
                                    .px(px(7.))
                                    .rounded(px(6.))
                                    .text_size(px(11.))
                                    .bg(a(theme::CRUST, 0.8)),
                            )
                        })
                        .when(look.selecting, |d| {
                            d.child(selection::pick_mark(look.selected).absolute().top(px(6.)).left(px(6.)))
                        }),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(2.))
                        .child(div().flex_1().min_w_0().truncate().font_weight(FontWeight::MEDIUM).child(format::time(clip.created)))
                        .child(kind_chip(clip.is_clip).flex_none())
                        .child(data(format::size(clip.size)).flex_none().text_size(px(12.)).text_color(c(theme::OVERLAY2))),
                ),
        )
        .child(
            div()
                .absolute()
                .top(px(6.))
                .right(px(6.))
                .flex()
                .gap(px(4.))
                .child(tile_button(format!("delete-{}", clip.key), group.clone(), "trash", theme::RED).on_click(cx.listener(
                    move |app, _, _, cx| {
                        cx.stop_propagation();
                        app.delete_clip(&delete, cx);
                    },
                )))
                .child(tile_button(format!("reveal-{}", clip.key), group, "folder", theme::TEXT).on_click(cx.listener(
                    move |app, _, _, cx| {
                        cx.stop_propagation();
                        app.reveal_clip(&reveal, cx);
                    },
                ))),
        )
        .into_any_element()
}

fn row(app: &FrameApp, clip: &Clip, index: usize, look: Look, cx: &mut Context<FrameApp>) -> AnyElement {
    let (open, reveal, delete) = (clip.clone(), clip.clone(), clip.clone());
    div()
        .id(SharedString::from(format!("row-{}", clip.key)))
        .relative()
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(10.))
        .py(px(8.))
        .when(index > 1, |d| d.border_t_1().border_color(theme::line()))
        .cursor_pointer()
        .when(look.selected, |d| d.bg(a(theme::MAUVE, 0.14)))
        .hover(|s| s.bg(a(theme::MAUVE, if look.selected { 0.2 } else { 0.08 })))
        .on_click(cx.listener(move |app, ev: &ClickEvent, _, cx| app.click_clip(&open, ev.modifiers(), cx)))
        .child(selection::tile_bounds(&app.selection.tiles, clip.key.clone()))
        .child(
            div()
                .relative()
                .flex_none()
                .child(thumb(app, clip, index).w(px(80.)).h(px(45.)).rounded(px(8.)))
                .when(look.selecting, |d| {
                    d.child(selection::pick_mark(look.selected).absolute().top(px(4.)).left(px(4.)))
                }),
        )
        .child(div().w(px(68.)).flex_none().font_weight(FontWeight::MEDIUM).child(format::time(clip.created)))
        .child(kind_chip(clip.is_clip).flex_none())
        .child(div().flex_1().min_w_0())
        .when_some(clip.duration_s, |d, len| {
            d.child(data(format::length(len)).flex_none().text_size(px(12.)).text_color(c(theme::SUBTEXT0)))
        })
        .child(
            data(format::size(clip.size))
                .min_w(px(52.))
                .flex_none()
                .flex()
                .justify_end()
                .text_size(px(12.))
                .text_color(c(theme::OVERLAY2)),
        )
        .child(
            div()
                .flex()
                .flex_none()
                .gap(px(2.))
                .child(row_button(format!("rowdelete-{}", clip.key), "trash", theme::SUBTEXT0).on_click(cx.listener(
                    move |app, _, _, cx| {
                        cx.stop_propagation();
                        app.delete_clip(&delete, cx);
                    },
                )))
                .child(row_button(format!("rowreveal-{}", clip.key), "folder", theme::SUBTEXT0).on_click(cx.listener(
                    move |app, _, _, cx| {
                        cx.stop_propagation();
                        app.reveal_clip(&reveal, cx);
                    },
                ))),
        )
        .into_any_element()
}

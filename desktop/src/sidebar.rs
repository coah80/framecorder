//! the left rail: wordmark, nav, your frame, the desktop update, the folder.

use gpui::{div, prelude::*, px, relative, AnyElement, Context, FontWeight};

use crate::app::{status_color, status_text, DesktopUpdate, FrameApp, Page};
use crate::theme::{self, a, c};
use crate::widgets::{data, dot, icon, spinner, wordmark};
use framecorder_app_lib::core::engine::State;
use gpui::{Animation, AnimationExt, SharedString};
use std::time::Duration;

pub fn render(app: &mut FrameApp, cx: &mut Context<FrameApp>) -> impl IntoElement {
    let count = app.clips.len();
    let clips_badge = (count > 0).then(|| data(count.to_string()).text_size(px(12.)).opacity(0.8).into_any_element());
    // a red dot by "frame" while one's recording, on every page
    let rec_badge = app.any_recording().then(|| dot(theme::RED, false).size(px(8.)).into_any_element());
    div()
        .w(px(216.))
        .flex_none()
        .h_full()
        .flex()
        .flex_col()
        .gap(px(24.))
        .pt(px(22.))
        .px(px(12.))
        .pb(px(14.))
        .bg(c(theme::MANTLE))
        .border_r_1()
        .border_color(theme::line())
        .child(div().px(px(12.)).child(wordmark()))
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(nav(app, cx, Page::Clips, "clips", "clips", clips_badge))
                .child(nav(app, cx, Page::Frame, "frame", "headset", rec_badge))
                .child(nav(app, cx, Page::Settings, "settings", "settings", None)),
        )
        .child(div().flex_1())
        .child(
            div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .children(frame_cards(app, cx))
                .children(desktop_pill(app, cx))
                .child(folder(app, cx)),
        )
}

fn nav(
    app: &FrameApp,
    cx: &mut Context<FrameApp>,
    page: Page,
    name: &'static str,
    icon_name: &str,
    badge: Option<AnyElement>,
) -> impl IntoElement {
    let on = app.page == page;
    let fg = if on { theme::MAUVE } else { theme::SUBTEXT0 };
    // ids are per window, and the clips page has a "clips" filter too. two
    // elements on one id share their click state and neither click lands
    div()
        .id(SharedString::from(format!("nav-{name}")))
        .flex()
        .items_center()
        .gap(px(12.))
        .h(px(44.))
        .px(px(12.))
        .rounded(px(10.))
        .font_weight(FontWeight::MEDIUM)
        .text_color(c(fg))
        .cursor_pointer()
        .when(on, |d| d.bg(a(theme::MAUVE, 0.1)))
        .when(!on, |d| d.hover(|s| s.bg(a(theme::TEXT, 0.05)).text_color(c(theme::TEXT))))
        .on_click(cx.listener(move |app, _, _, cx| app.go(page, cx)))
        .child(icon(icon_name, 18., c(fg)))
        .child(div().flex_1().child(name))
        .children(badge)
}

fn frame_cards(app: &FrameApp, cx: &mut Context<FrameApp>) -> Vec<AnyElement> {
    app.statuses
        .iter()
        .map(|s| {
            let fp = s.fingerprint.clone();
            let updating = app.frame_updating.contains(&s.fingerprint) || s.update.as_ref().is_some_and(|u| u.updating);
            let update = s.update.as_ref().filter(|u| u.available && !updating);
            let recording = s.state == State::Connected && app.live(&s.fingerprint).is_some_and(|l| l.recording());
            let color = if recording { theme::RED } else { status_color(s) };
            let open = fp.clone();
            div()
                .id(SharedString::from(format!("frame-{fp}")))
                .flex()
                .flex_col()
                .gap(px(6.))
                .p(px(14.))
                .rounded(px(14.))
                .bg(c(theme::BASE))
                .border_1()
                .border_color(a(theme::SURFACE1, 0.6))
                .cursor_pointer()
                .hover(|s| s.border_color(c(theme::SURFACE2)))
                .when(matches!(s.state, State::Unreachable | State::Full), |d| {
                    d.bg(a(theme::RED, 0.06)).border_color(a(theme::RED, 0.28))
                })
                .on_click(cx.listener(move |app, _, _, cx| app.pick_frame(open.clone(), cx)))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .child(dot(color, s.state == State::Connected))
                        .child(div().flex_1().truncate().font_weight(FontWeight::MEDIUM).child(s.name.clone())),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(c(if color == theme::RED { theme::RED } else { theme::SUBTEXT0 }))
                        .child(if recording { "recording" } else { status_text(s) }),
                )
                .when(updating, |d| d.child(updating_text(format!("upd-{fp}"))))
                .when_some(update.cloned(), |d, u| {
                    d.child(
                        div()
                            .id(format!("update-{fp}"))
                            .flex()
                            .items_center()
                            .justify_between()
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(c(theme::MAUVE))
                            .cursor_pointer()
                            .hover(|s| s.text_color(c(theme::MAUVE_LIGHT)))
                            .on_click(cx.listener(move |app, _, _, cx| {
                                // the card around it opens the frame page, this shouldn't
                                cx.stop_propagation();
                                app.update_frame(fp.clone(), cx);
                            }))
                            .child("update frame app")
                            .when_some(u.latest.clone(), |d, v| d.child(data(v).text_color(c(theme::MAUVE_LIGHT)))),
                    )
                })
                .into_any_element()
        })
        .collect()
}

fn desktop_pill(app: &FrameApp, cx: &mut Context<FrameApp>) -> Option<AnyElement> {
    let (icon_name, text, right, fill) = match &app.desktop {
        DesktopUpdate::None => return None,
        DesktopUpdate::Available(rel) => ("download", "update this app", rel.version.clone(), 0.0),
        DesktopUpdate::Downloading { pct, .. } => {
            ("download", "downloading", format!("{}%", (pct * 100.) as u32), *pct)
        }
        DesktopUpdate::Ready(_) => ("restart", "restart to finish", String::new(), 1.0),
    };
    let downloading = matches!(app.desktop, DesktopUpdate::Downloading { .. });
    Some(
        div()
            .id("desktop-update")
            .relative()
            .overflow_hidden()
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(34.))
            .px(px(12.))
            .rounded_full()
            .border_1()
            .border_color(a(theme::MAUVE, 0.3))
            .bg(a(theme::MAUVE, 0.1))
            .text_color(c(theme::MAUVE))
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .cursor_pointer()
            .hover(|s| s.bg(a(theme::MAUVE, 0.16)))
            .on_click(cx.listener(|app, _, _, cx| app.desktop_update_clicked(cx)))
            .child(div().absolute().top_0().bottom_0().left_0().w(relative(fill)).bg(a(theme::MAUVE, 0.22)))
            .child(if downloading {
                spinner("desktop-dl", 14., c(theme::MAUVE)).into_any_element()
            } else {
                icon(icon_name, 14., c(theme::MAUVE)).into_any_element()
            })
            .child(div().flex_1().child(text))
            .child(data(right).text_color(c(theme::MAUVE_LIGHT)))
            .into_any_element(),
    )
}

/// ~/Videos/framecorder rather than the whole path
pub fn short_path(p: &std::path::Path) -> String {
    match dirs::home_dir().and_then(|h| p.strip_prefix(h).ok().map(|r| r.to_path_buf())) {
        Some(rest) => format!("~/{}", rest.display()),
        None => p.display().to_string(),
    }
}

fn folder(app: &FrameApp, cx: &mut Context<FrameApp>) -> impl IntoElement {
    div()
        .id("open-folder")
        .flex()
        .items_center()
        .gap(px(10.))
        .h(px(44.))
        .px(px(12.))
        .rounded(px(10.))
        .text_color(c(theme::SUBTEXT0))
        .cursor_pointer()
        .hover(|s| s.bg(c(theme::SURFACE1)).text_color(c(theme::TEXT)))
        .on_click(cx.listener(|app, _, _, cx| app.open_folder(cx)))
        .child(icon("folder", 18., c(theme::SUBTEXT0)))
        .child(
            div()
                .flex()
                .flex_col()
                .min_w_0()
                .child(
                    div().text_size(px(13.)).font_weight(FontWeight::MEDIUM).line_height(px(17.)).child("open folder"),
                )
                .child(
                    data(short_path(&app.core.download_dir))
                        .text_size(px(11.))
                        .line_height(px(15.))
                        .text_color(c(theme::OVERLAY2))
                        .truncate(),
                ),
        )
}

/// "updating", grey, filling in mauve from the left over and over, like a bar
/// made of text. the frame doesn't say how far along it is, it just comes back
fn updating_text(id: impl Into<SharedString>) -> impl IntoElement {
    let text = || div().text_size(px(12.)).font_weight(FontWeight::MEDIUM).whitespace_nowrap().child("updating");
    div().relative().child(text().text_color(c(0x6c7086))).child(
        div().absolute().top_0().left_0().overflow_hidden().child(text().text_color(c(theme::MAUVE))).with_animation(
            id.into(),
            Animation::new(Duration::from_millis(1800)).repeat(),
            |d, delta| d.w(relative(delta)),
        ),
    )
}

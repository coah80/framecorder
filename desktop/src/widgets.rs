//! the small pieces every screen uses.

use std::time::Duration;

use gpui::{
    div, percentage, prelude::*, px, svg, Animation, AnimationExt, Div, ElementId, FontWeight, Rgba, SharedString,
    Stateful, Svg, Transformation,
};

use crate::assets::icon_path;
use crate::theme::{self, a, c};

pub fn icon(name: &str, size: f32, color: Rgba) -> Svg {
    svg().path(icon_path(name)).size(px(size)).flex_none().text_color(color)
}

pub fn spinner(id: impl Into<ElementId>, size: f32, color: Rgba) -> impl IntoElement {
    icon("spinner", size, color).with_animation(id, Animation::new(Duration::from_millis(1100)).repeat(), |s, delta| {
        s.with_transformation(Transformation::rotate(percentage(delta)))
    })
}

/// the little caps label over a group, "TODAY", "FRAMES"
pub fn label(text: &str) -> Div {
    div()
        .font_family(theme::HEADING)
        .font_weight(FontWeight::BOLD)
        .text_size(px(11.))
        .letter_spacing(px(1.3))
        .text_color(c(theme::OVERLAY2))
        .child(text.to_uppercase())
}

/// a screen's title, with the mauve dot
pub fn title(text: &str, size: f32) -> Div {
    div()
        .flex()
        .font_family(theme::HEADING)
        .font_weight(FontWeight::BOLD)
        .text_size(px(size))
        .line_height(px(size * 1.15))
        .child(text.to_string())
        .child(div().text_color(c(theme::MAUVE)).child("."))
}

pub fn wordmark() -> Div {
    title("framecorder", 19.)
}

pub fn data(text: impl Into<SharedString>) -> Div {
    div().font_family(theme::DATA).child(text.into())
}

pub fn dot(color: u32, ring: bool) -> Div {
    let d = div().size(px(10.)).flex_none().rounded_full().bg(c(color));
    if ring {
        // a soft halo round a live connection
        div()
            .size(px(18.))
            .flex_none()
            .m(px(-4.))
            .rounded_full()
            .bg(a(color, 0.16))
            .flex()
            .items_center()
            .justify_center()
            .child(d)
    } else {
        d
    }
}

pub fn chip(text: &str, bg: Rgba, fg: Rgba) -> Div {
    div()
        .px(px(8.))
        .rounded_full()
        .text_size(px(11.))
        .font_weight(FontWeight::MEDIUM)
        .bg(bg)
        .text_color(fg)
        .child(text.to_string())
}

pub fn kind_chip(is_clip: bool) -> Div {
    if is_clip {
        chip("clip", a(theme::MAUVE, 0.12), c(theme::MAUVE))
    } else {
        chip("recording", a(0x6c7086, 0.2), c(theme::SUBTEXT1))
    }
}

/// the filled mauve button
pub fn primary(id: impl Into<ElementId>, text: &str) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .h(px(40.))
        .px(px(18.))
        .rounded(px(12.))
        .bg(c(theme::MAUVE))
        .text_color(c(theme::CRUST))
        .font_family(theme::HEADING)
        .font_weight(FontWeight::BOLD)
        .text_size(px(14.))
        .cursor_pointer()
        .hover(|s| s.bg(c(theme::MAUVE_LIGHT)))
        .child(text.to_string())
}

/// the quiet bordered button
pub fn outline(id: impl Into<ElementId>, text: &str) -> Stateful<Div> {
    outline_base(id, text)
        .bg(a(theme::SURFACE0, 0.6))
        .hover(|s| s.bg(c(theme::SURFACE1)).border_color(c(theme::SURFACE2)))
}

/// the bordered button for something you can't take back, red on hover
pub fn danger(id: impl Into<ElementId>, text: &str) -> Stateful<Div> {
    outline_base(id, text).hover(|s| s.text_color(c(theme::RED)).border_color(c(theme::RED)).bg(a(theme::RED, 0.08)))
}

fn outline_base(id: impl Into<ElementId>, text: &str) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .gap(px(8.))
        .h(px(36.))
        .px(px(14.))
        .rounded(px(10.))
        .border_1()
        .border_color(a(theme::SURFACE1, 0.75))
        .text_size(px(13.))
        .text_color(c(theme::TEXT))
        .cursor_pointer()
        .child(text.to_string())
}

pub fn switch(id: impl Into<ElementId>, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex_none()
        .w(px(44.))
        .h(px(26.))
        .p(px(3.))
        .rounded_full()
        .flex()
        .when(on, |d| d.justify_end())
        .bg(c(if on { theme::MAUVE } else { theme::SURFACE1 }))
        .cursor_pointer()
        .child(div().size(px(20.)).rounded_full().bg(c(if on { theme::CRUST } else { theme::TEXT })))
}

/// a card: the faint fill and hairline border everything sits in
pub fn card() -> Div {
    div().rounded(px(14.)).bg(theme::card()).border_1().border_color(theme::card_line())
}

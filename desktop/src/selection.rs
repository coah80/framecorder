//! picking several clips at once, to delete them or show them in their folder
//! together. ctrl-click (cmd on a mac) picks one, shift-click the range from
//! the last one clicked, and a drag across the library picks everything it
//! crosses, the way a swipe does on the phone. the bar at the bottom says
//! what's picked and what to do with it.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    canvas, div, point, prelude::*, px, AnyElement, Bounds, Context, Div, FontWeight, Modifiers, MouseDownEvent,
    MouseMoveEvent, Pixels, Point, Task, Window,
};

use crate::app::FrameApp;
use crate::clips;
use crate::sync::Clip;
use crate::theme::{self, a, c};
use crate::widgets::{data, destructive, icon, outline};
use crate::format;

/// where the tiles were painted last frame, in the library's order, so a
/// press or a drag knows which clip it's on
pub type Tiles = Rc<RefCell<Vec<(String, Bounds<Pixels>)>>>;

/// how often a drag looks at the mouse, to scroll at the edges
const TICK: Duration = Duration::from_millis(16);
/// how close to the gallery's top or bottom a drag starts scrolling, and the most it moves per tick
const EDGE: f32 = 48.;
const STEP: f32 = 18.;

pub struct Selection {
    /// the keys of the clips picked. only the ones on screen count, so one
    /// that's gone, or hidden by the search or the filter, can't go by accident
    pub picked: HashSet<String>,
    /// the clip clicked last, where shift-click's range starts
    pub anchor: Option<String>,
    /// a press in the library that may turn into a drag
    pub drag: Option<Drag>,
    /// the question before the picked clips go to the trash
    pub asking: bool,
    pub tiles: Tiles,
    /// the drag the last mouse-up ended. the mouse-up is seen before the
    /// click it makes, so the click still knows whether it was a drag
    ended: Option<Drag>,
    /// the once-a-frame look while dragging, see `drag_tick`
    ticker: Option<Task<()>>,
}

pub struct Drag {
    /// the clip the drag started on, once it's crossed one
    from: Option<String>,
    /// what was picked before, the range goes on top of it
    base: HashSet<String>,
    /// picking the range, or letting it go (a drag from a clip that was picked)
    adding: bool,
    /// the keys held when the mouse went down, so the click that may follow sees the same ones
    modifiers: Modifiers,
    /// whether it's crossed a clip yet. once it has, the click at the end isn't one
    moved: bool,
}

impl Selection {
    pub fn new() -> Self {
        Self {
            picked: HashSet::new(),
            anchor: None,
            drag: None,
            asking: false,
            tiles: Rc::default(),
            ended: None,
            ticker: None,
        }
    }
}

/// the keys from one clip to another in the library's order, either way round.
/// nothing if either isn't shown
fn between<'a>(order: &'a [String], from: &str, to: &str) -> &'a [String] {
    let (Some(i), Some(j)) = (order.iter().position(|k| k == from), order.iter().position(|k| k == to)) else {
        return &[];
    };
    &order[i.min(j)..=i.max(j)]
}

/// what's picked after a drag: the range from `from` to `to` added to (or taken out of) what was picked before
fn after_drag(base: &HashSet<String>, order: &[String], from: &str, to: &str, adding: bool) -> HashSet<String> {
    let range = between(order, from, to);
    if adding {
        base.iter().chain(range).cloned().collect()
    } else {
        base.iter().filter(|k| !range.contains(k)).cloned().collect()
    }
}

/// which tile a point is on, if it's inside the gallery. a tile scrolled up
/// under the header still has bounds, but the mouse isn't on it
fn tile_at(tiles: &[(String, Bounds<Pixels>)], gallery: &Bounds<Pixels>, at: Point<Pixels>) -> Option<usize> {
    if !gallery.contains(&at) {
        return None;
    }
    tiles.iter().position(|(_, b)| b.contains(&at))
}

/// how far to scroll this tick: nothing in the middle, more the closer the
/// mouse is to an edge, and full speed past it
fn edge_step(top: f32, bottom: f32, y: f32) -> f32 {
    if y < top + EDGE {
        -STEP * ((top + EDGE - y) / EDGE).min(1.)
    } else if y > bottom - EDGE {
        STEP * ((y - bottom + EDGE) / EDGE).min(1.)
    } else {
        0.
    }
}

/// "3 clips", "2 recordings", "5 clips and recordings"
fn what(clips: &[Clip]) -> String {
    let n = clips.len();
    let (clip, rec) = (n == 1, clips.iter().filter(|c| c.is_clip).count());
    match (rec, n - rec) {
        (_, 0) => format!("{n} clip{}", if clip { "" } else { "s" }),
        (0, _) => format!("{n} recording{}", if clip { "" } else { "s" }),
        _ => format!("{n} clips and recordings"),
    }
}

/// "3 selected · 44 MB"
fn selected_text(clips: &[Clip]) -> String {
    format!("{} selected · {}", clips.len(), format::size(clips.iter().map(|c| c.size).sum()))
}

fn bin() -> &'static str {
    if cfg!(windows) {
        "the recycle bin"
    } else {
        "the trash"
    }
}

impl FrameApp {
    /// the library's order, for ranges and select all
    fn order(&self) -> Vec<String> {
        clips::shown(self, chrono::Local::now().date_naive()).into_iter().map(|c| c.key.clone()).collect()
    }

    /// the picked clips that are on screen, in the library's order
    pub fn selected(&self) -> Vec<Clip> {
        let today = chrono::Local::now().date_naive();
        clips::shown(self, today).into_iter().filter(|c| self.selection.picked.contains(&c.key)).cloned().collect()
    }

    pub fn selecting(&self) -> bool {
        !self.selected().is_empty()
    }

    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.selection.picked.clear();
        self.selection.anchor = None;
        self.selection.asking = false;
        self.selection.drag = None;
        self.selection.ended = None;
        self.selection.ticker = None;
        cx.notify();
    }

    /// everything on screen, or nothing again once it all is
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        let all = self.order();
        if all.iter().all(|k| self.selection.picked.contains(k)) {
            self.clear_selection(cx);
            return;
        }
        self.selection.picked = all.into_iter().collect();
        self.selection.asking = false;
        cx.notify();
    }

    /// a click on a clip. shift picks from the last one clicked to it, ctrl
    /// (cmd on a mac) picks it or lets it go, and while anything's picked a
    /// plain click does the same instead of opening
    pub fn click_clip(&mut self, clip: &Clip, modifiers: Modifiers, cx: &mut Context<Self>) {
        let drag = self.selection.drag.as_ref().or(self.selection.ended.as_ref());
        if drag.is_some_and(|d| d.moved) {
            return;
        }
        let mods = drag.map(|d| d.modifiers).unwrap_or(modifiers);
        if mods.shift {
            let order = self.order();
            let from = self.selection.anchor.clone().unwrap_or_else(|| clip.key.clone());
            self.selection.picked.extend(between(&order, &from, &clip.key).iter().cloned());
            self.selection.anchor.get_or_insert_with(|| clip.key.clone());
        } else if mods.secondary() || self.selecting() {
            if !self.selection.picked.remove(&clip.key) {
                self.selection.picked.insert(clip.key.clone());
            }
            self.selection.anchor = Some(clip.key.clone());
        } else {
            self.open_clip(clip, cx);
            return;
        }
        self.selection.asking = false;
        cx.notify();
    }

    fn tile_under(&self, at: Point<Pixels>) -> Option<String> {
        let tiles = self.selection.tiles.borrow();
        tile_at(&tiles, &self.gallery.bounds(), at).map(|i| tiles[i].0.clone())
    }

    /// the mouse going down in the library. on a clip while picking (or with
    /// ctrl or shift), or on nothing, it may be the start of a drag. a plain
    /// press on a clip when nothing's picked is the click that opens it
    pub fn press(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.selection.ended = None;
        let base = self.selection.picked.clone();
        let drag = match self.tile_under(ev.position) {
            Some(key) if self.selecting() || ev.modifiers.secondary() || ev.modifiers.shift => Drag {
                adding: ev.modifiers.shift || !base.contains(&key),
                from: if ev.modifiers.shift { self.selection.anchor.clone().or(Some(key)) } else { Some(key) },
                base,
                modifiers: ev.modifiers,
                moved: false,
            },
            Some(_) => return,
            None => Drag { from: None, base, adding: true, modifiers: ev.modifiers, moved: false },
        };
        self.selection.drag = Some(drag);
        self.selection.ticker = Some(cx.spawn_in(window, async move |this, cx| loop {
            cx.background_executor().timer(TICK).await;
            if this.update_in(cx, |app, window, cx| app.drag_tick(window, cx)).is_err() {
                break;
            }
        }));
    }

    /// the mouse moving over the gallery with the button down
    pub fn drag_moved(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        if self.selection.drag.is_none() {
            return;
        }
        // a release we never saw, outside the window
        if !ev.dragging() {
            self.release(cx);
            return;
        }
        self.drag_to(ev.position, cx);
    }

    fn drag_to(&mut self, at: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(key) = self.tile_under(at) else { return };
        let order = self.order();
        let Some(drag) = &mut self.selection.drag else { return };
        let from = drag.from.get_or_insert_with(|| key.clone()).clone();
        drag.moved = true;
        let picked = after_drag(&drag.base, &order, &from, &key, drag.adding);
        if picked != self.selection.picked {
            self.selection.picked = picked;
            self.selection.asking = false;
            cx.notify();
        }
    }

    /// every tick while dragging: near the gallery's top or bottom the view
    /// scrolls, and the range follows what's under the mouse now
    fn drag_tick(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.selection.drag.is_none() {
            return;
        }
        let at = window.mouse_position();
        let view = self.gallery.bounds();
        let step = edge_step(view.top().as_f32(), view.bottom().as_f32(), at.y.as_f32());
        if step == 0. {
            return;
        }
        let (offset, most) = (self.gallery.offset(), self.gallery.max_offset());
        let y = (offset.y.as_f32() - step).clamp(-most.y.as_f32(), 0.);
        if y == offset.y.as_f32() {
            return;
        }
        self.gallery.set_offset(point(offset.x, px(y)));
        // the tiles are still where last frame put them, the next tick sees them moved
        self.drag_to(at, cx);
        cx.notify();
    }

    /// the mouse coming up, anywhere. seen before the click it makes, so a
    /// press on a tile's button can't leave the ticker running
    pub fn release(&mut self, cx: &mut Context<Self>) {
        self.selection.ticker = None;
        if let Some(drag) = self.selection.drag.take() {
            if drag.moved {
                cx.notify();
            }
            self.selection.ended = Some(drag);
        }
    }

    pub fn ask_delete_selected(&mut self, cx: &mut Context<Self>) {
        self.selection.asking = true;
        cx.notify();
    }

    pub fn keep_selected(&mut self, cx: &mut Context<Self>) {
        self.selection.asking = false;
        cx.notify();
    }

    /// the picked clips go to the trash the way one does, in one go
    pub fn delete_selected(&mut self, cx: &mut Context<Self>) {
        let picked = self.selected();
        self.selection.asking = false;
        let said = what(&picked);
        if self.demo {
            self.clips = self.clips.iter().filter(|c| !self.selection.picked.contains(&c.key)).cloned().collect();
            self.clear_selection(cx);
            self.toast(format!("moved {said} to {}", bin()), cx);
            return;
        }
        let (there, gone): (Vec<&Clip>, Vec<&Clip>) = picked.iter().partition(|c| c.location.exists());
        let keys: Vec<String> = there.iter().map(|c| c.key.clone()).collect();
        let paths: Vec<_> = there.iter().map(|c| c.location.clone()).collect();
        let missing = gone.len();
        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(async move {
                    if paths.is_empty() {
                        return Ok(());
                    }
                    trash::delete_all(&paths).map_err(|e| e.to_string())
                })
                .await;
            let _ = this.update(cx, |app, cx| {
                match res {
                    Ok(()) => {
                        app.clips = app.clips.iter().filter(|c| !keys.contains(&c.key)).cloned().collect();
                        app.clear_selection(cx);
                        let note = if missing > 0 { format!(", {missing} had already been moved or deleted") } else { String::new() };
                        app.toast(format!("moved {said} to {}{note}", bin()), cx);
                    }
                    Err(e) => app.toast(format!("couldn't delete them: {e}"), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// one folder window, on the first picked clip that's still there
    pub fn reveal_selected(&mut self, cx: &mut Context<Self>) {
        match self.selected().iter().find(|c| c.location.exists()) {
            Some(clip) => cx.reveal_path(&clip.location),
            None => self.toast("those files have been moved or deleted", cx),
        }
    }
}

/// the circle in a tile's corner while picking: hollow until the clip's picked, then mauve with a check
pub fn pick_mark(selected: bool) -> Div {
    div()
        .size(px(22.))
        .flex_none()
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(if selected { c(theme::MAUVE) } else { a(theme::CRUST, 0.4) })
        .border_1()
        .border_color(if selected { c(theme::MAUVE) } else { a(theme::TEXT, 0.85) })
        .when(selected, |d| d.child(icon("check", 13., c(theme::CRUST))))
}

/// an invisible layer over a tile that notes where it was painted, for presses and drags
pub fn tile_bounds(tiles: &Tiles, key: String) -> impl IntoElement {
    let tiles = tiles.clone();
    canvas(move |bounds, _, _| tiles.borrow_mut().push((key, bounds)), |_, _, _, _| {}).absolute().inset_0()
}

/// the bar over the bottom of the library while anything's picked: the
/// count, and what to do with them. it asks once before deleting
pub fn bar(app: &FrameApp, selected: &[Clip], all: bool, cx: &mut Context<FrameApp>) -> AnyElement {
    let asking = app.selection.asking;
    let tint = if asking { theme::RED } else { theme::MAUVE };
    div()
        .id("selection")
        .occlude()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap(px(10.))
        .px(px(12.))
        .py(px(10.))
        .rounded(px(14.))
        .bg(c(theme::SURFACE0))
        .border_1()
        .border_color(a(tint, 0.45))
        .shadow_lg()
        .when(asking, |d| d.on_mouse_down_out(cx.listener(|app, _, _, cx| app.keep_selected(cx))))
        .child(if asking { question(selected) } else { count(selected, cx) })
        .child(if asking { answers(cx) } else { actions(all, cx) })
        .into_any_element()
}

fn count(selected: &[Clip], cx: &mut Context<FrameApp>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(10.))
        .child(
            div()
                .id("selection-clear")
                .size(px(32.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(9.))
                .cursor_pointer()
                .hover(|s| s.bg(c(theme::SURFACE1)))
                .on_click(cx.listener(|app, _, _, cx| app.clear_selection(cx)))
                .child(icon("x", 15., c(theme::SUBTEXT0))),
        )
        .child(data(selected_text(selected)).font_weight(FontWeight::MEDIUM).text_color(c(theme::MAUVE)))
}

fn actions(all: bool, cx: &mut Context<FrameApp>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(
            outline("selection-all", if all { "select none" } else { "select all" })
                .on_click(cx.listener(|app, _, _, cx| app.select_all(cx))),
        )
        .child(
            outline("selection-reveal", "show in folder")
                .child(icon("folder", 15., c(theme::SUBTEXT1)))
                .on_click(cx.listener(|app, _, _, cx| app.reveal_selected(cx))),
        )
        .child(
            destructive("selection-delete", "delete")
                .child(icon("trash", 15., c(theme::RED)))
                .on_click(cx.listener(|app, _, _, cx| app.ask_delete_selected(cx))),
        )
}

fn question(selected: &[Clip]) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(12.))
        .child(
            div()
                .size(px(32.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(9.))
                .bg(a(theme::RED, 0.12))
                .child(icon("trash", 16., c(theme::RED))),
        )
        .child(
            div()
                .flex()
                .flex_col()
                .child(
                    div()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(c(theme::RED))
                        .child(format!("move {} to {}?", what(selected), bin())),
                )
                .child(div().text_size(px(12.)).text_color(c(theme::SUBTEXT1)).child("they can come back from there.")),
        )
}

fn answers(cx: &mut Context<FrameApp>) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .child(outline("selection-keep", "keep them").on_click(cx.listener(|app, _, _, cx| app.keep_selected(cx))))
        .child(
            destructive("selection-confirm", "delete").on_click(cx.listener(|app, _, _, cx| app.delete_selected(cx))),
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::size;
    use std::path::PathBuf;

    fn keys(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("demo/{i}")).collect()
    }

    #[test]
    fn a_range_runs_either_way_and_needs_both_ends_on_screen() {
        let order = keys(6);
        assert_eq!(between(&order, "demo/1", "demo/3"), &order[1..=3]);
        assert_eq!(between(&order, "demo/3", "demo/1"), &order[1..=3]);
        assert_eq!(between(&order, "demo/2", "demo/2"), &order[2..=2]);
        assert!(between(&order, "demo/2", "gone").is_empty());
    }

    #[test]
    fn a_drag_adds_a_range_to_what_was_picked_or_takes_one_out() {
        let order = keys(6);
        let base: HashSet<String> = ["demo/0".to_string(), "demo/4".to_string()].into();
        let picked = after_drag(&base, &order, "demo/2", "demo/3", true);
        assert_eq!(picked.len(), 4);
        assert!(picked.contains("demo/0") && picked.contains("demo/2") && picked.contains("demo/3"));
        // dragging back over a shorter range leaves what the longer one picked
        let picked = after_drag(&base, &order, "demo/2", "demo/2", true);
        assert_eq!(picked.len(), 3);
        // from a picked clip, the drag lets the range go
        let picked = after_drag(&base, &order, "demo/4", "demo/5", false);
        assert_eq!(picked, ["demo/0".to_string()].into());
    }

    #[test]
    fn a_tile_scrolled_under_the_header_is_not_under_the_mouse() {
        let tile = |y: f32| Bounds::new(point(px(0.), px(y)), size(px(100.), px(80.)));
        let tiles = vec![("a".to_string(), tile(-40.)), ("b".to_string(), tile(60.))];
        let gallery = Bounds::new(point(px(0.), px(50.)), size(px(300.), px(400.)));
        assert_eq!(tile_at(&tiles, &gallery, point(px(10.), px(20.))), None);
        assert_eq!(tile_at(&tiles, &gallery, point(px(10.), px(70.))), Some(1));
        assert_eq!(tile_at(&tiles, &gallery, point(px(10.), px(200.))), None);
    }

    #[test]
    fn the_edge_scrolls_and_the_middle_does_not() {
        assert_eq!(edge_step(100., 500., 300.), 0.);
        assert!(edge_step(100., 500., 110.) < 0.);
        assert_eq!(edge_step(100., 500., 0.), -STEP);
        assert_eq!(edge_step(100., 500., 600.), STEP);
    }

    #[test]
    fn says_what_is_picked() {
        let clip = |is_clip: bool| Clip {
            key: "k".into(),
            host: "h".into(),
            name: "n".into(),
            is_clip,
            size: 48_000_000,
            created: 0,
            duration_s: None,
            synced_at: 0,
            location: PathBuf::from("/nonexistent"),
        };
        assert_eq!(what(&[clip(true)]), "1 clip");
        assert_eq!(what(&[clip(false), clip(false)]), "2 recordings");
        assert_eq!(what(&[clip(true), clip(false)]), "2 clips and recordings");
        assert_eq!(selected_text(&[clip(true), clip(true)]), "2 selected · 96 MB");
    }
}

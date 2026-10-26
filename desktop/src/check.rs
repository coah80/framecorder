//! the connection check: the real checks a frame has to pass, one by one,
//! and what each one saw. for when "can't reach it" isn't enough to go on.

use std::sync::Arc;
use std::time::Duration;

use framecorder_app_lib::core::api::Client;
use framecorder_app_lib::core::discover;
use framecorder_app_lib::core::engine::{Engine, State};
use framecorder_app_lib::core::store::Host;
use gpui::{div, prelude::*, px, AnyElement, Context, Div, FontWeight, SharedString, Task};

use crate::app::{FrameApp, Page};
use crate::theme::{self, a, c};
use crate::widgets::{card, icon, outline, primary, spinner, title};
use crate::{format, sync};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Waiting,
    Running,
    Pass,
    Fail,
    Skipped,
}

#[derive(Clone)]
pub struct Step {
    pub title: String,
    pub detail: String,
    pub outcome: Outcome,
}

/// one run of the checks for one frame
pub struct Check {
    pub fingerprint: String,
    pub steps: Vec<Step>,
    pub running: bool,
    pub task: Option<Task<()>>,
}

const BROWSE: Duration = Duration::from_secs(6);
const SWEEP: Duration = Duration::from_secs(8);

impl FrameApp {
    pub fn open_check(&mut self, fingerprint: String, cx: &mut Context<Self>) {
        self.frame = Some(fingerprint.clone());
        self.check = Some(Check { fingerprint, steps: Vec::new(), running: false, task: None });
        self.page = Page::Check;
        self.run_check(cx);
        cx.notify();
    }

    pub fn close_check(&mut self, cx: &mut Context<Self>) {
        self.check = None;
        self.go(Page::Frame, cx);
    }

    /// runs the checks on the sync runtime and shows each one as it lands
    pub fn run_check(&mut self, cx: &mut Context<Self>) {
        let Some(fingerprint) = self.check.as_ref().filter(|c| !c.running).map(|c| c.fingerprint.clone()) else {
            return;
        };
        // a demo's steps are made up, and stay
        let Some(host) = (!self.demo).then(|| self.core.engine.host(&fingerprint)).flatten() else {
            return;
        };
        let (tx, rx) = async_channel::unbounded::<(usize, Step)>();
        let engine = self.core.engine.clone();
        let job = self.core.rt.spawn(async move { checks(engine, host, tx).await });
        let task = cx.spawn(async move |this, cx| {
            while let Ok((i, step)) = rx.recv().await {
                let alive = this.update(cx, |app, cx| {
                    if let Some(check) = &mut app.check {
                        if i < check.steps.len() {
                            check.steps[i] = step;
                        } else {
                            check.steps.push(step);
                        }
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }
            let _ = job.await;
            let _ = this.update(cx, |app, cx| {
                if let Some(check) = &mut app.check {
                    check.running = false;
                }
                cx.notify();
            });
        });
        if let Some(check) = &mut self.check {
            check.steps.clear();
            check.running = true;
            check.task = Some(task);
        }
        cx.notify();
    }
}

/// the networks a sweep covers, like "192.168.1.x"
fn networks() -> Vec<String> {
    let mut nets: Vec<String> = discover::sweep_targets()
        .iter()
        .map(|ip| {
            let o = ip.octets();
            format!("{}.{}.{}.x", o[0], o[1], o[2])
        })
        .collect();
    nets.dedup();
    nets
}

fn ip_of(addr: &str) -> String {
    addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(addr).to_string()
}

async fn checks(engine: Arc<Engine>, host: Host, tx: async_channel::Sender<(usize, Step)>) {
    let put = |i: usize, title: &str, detail: String, outcome: Outcome| {
        let _ = tx.try_send((i, Step { title: title.into(), detail, outcome }));
    };

    let nets = networks();
    let online = !nets.is_empty();
    put(
        0,
        "this computer is on a network",
        if online { nets.join(", ") } else { "no local network found. is it on wi-fi, or plugged in?".into() },
        if online { Outcome::Pass } else { Outcome::Fail },
    );

    let answers = format!("answers at {}", ip_of(&host.addr));
    put(1, &answers, "asking...".into(), Outcome::Running);
    let problem = match Client::pinned(&host.addr, &host.fingerprint, engine.app()) {
        Ok(client) => client.with_token(&host.token).hello().await.err().map(|e| e.to_string()),
        Err(e) => Some(e.to_string()),
    };
    match &problem {
        None => put(1, &answers, "it's there and it's the frame you paired".into(), Outcome::Pass),
        Some(e) => put(1, &answers, e.clone(), Outcome::Fail),
    }

    const BY_NAME: &str = "found by name";
    const BY_KNOCKING: &str = "found by asking every address";
    if problem.is_none() {
        put(2, BY_NAME, "no need, it answered".into(), Outcome::Skipped);
        put(3, BY_KNOCKING, "no need, it answered".into(), Outcome::Skipped);
        return;
    }

    // only worth looking for if it isn't where it was
    put(2, BY_NAME, "asking the network...".into(), Outcome::Running);
    put(3, BY_KNOCKING, format!("checking {}...", nets.join(", ")), Outcome::Waiting);
    let mut by_name = None;
    let _ = discover::browse(BROWSE, |f| {
        let it = f.fingerprint == host.fingerprint;
        if it {
            by_name = Some(f);
        }
        !it
    })
    .await;
    match &by_name {
        Some(f) => put(2, BY_NAME, format!("at {}", ip_of(&f.addr)), Outcome::Pass),
        None => put(
            2,
            BY_NAME,
            "no answer. some routers and firewalls block this, the next check doesn't need it.".into(),
            Outcome::Fail,
        ),
    }

    put(3, BY_KNOCKING, format!("checking {}...", nets.join(", ")), Outcome::Running);
    let mut direct = None;
    discover::sweep(SWEEP, engine.app(), |f| {
        let it = f.fingerprint == host.fingerprint;
        if it {
            direct = Some(f);
        }
        !it
    })
    .await;
    match &direct {
        Some(f) => put(3, BY_KNOCKING, format!("at {}", ip_of(&f.addr)), Outcome::Pass),
        None => put(3, BY_KNOCKING, "no frame answered on this network".into(), Outcome::Fail),
    }
    if by_name.is_some() || direct.is_some() {
        engine.retry_now();
    }
}

// the page

pub fn render(app: &mut FrameApp, cx: &mut Context<FrameApp>) -> impl IntoElement {
    let Some(check) = &app.check else {
        return div().into_any_element();
    };
    let status = app.statuses.iter().find(|s| s.fingerprint == check.fingerprint);
    let name = status.map(|s| s.name.clone()).unwrap_or_else(|| "your frame".into());
    let connected = status.is_some_and(|s| s.state == State::Connected);
    let trouble = !connected && check.steps.iter().any(|s| s.outcome == Outcome::Fail);
    let synced = sync::last_sync(&app.clips, &check.fingerprint).map(|t| format!("last synced {}. ", format::ago(t)));
    let tint = if connected { theme::GREEN } else { theme::RED };
    let fp = check.fingerprint.clone();
    let rows: Vec<AnyElement> = check.steps.iter().enumerate().map(|(i, s)| row(s, i == 0)).collect();

    div()
        .id("check-page")
        .flex_1()
        .min_w_0()
        .h_full()
        .overflow_y_scroll()
        .px(px(28.))
        .pt(px(22.))
        .pb(px(32.))
        .flex()
        .flex_col()
        .gap(px(18.))
        .child(
            div()
                .id("check-back")
                .flex()
                .items_center()
                .gap(px(6.))
                .text_size(px(13.))
                .text_color(c(theme::MAUVE))
                .cursor_pointer()
                .hover(|s| s.text_color(c(theme::MAUVE_LIGHT)))
                .on_click(cx.listener(|app, _, _, cx| app.close_check(cx)))
                .child(icon("back", 14., c(theme::MAUVE)))
                .child("frame"),
        )
        .child(title("connection check", 26.))
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(16.))
                .child(
                    div()
                        .size(px(52.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(16.))
                        .bg(a(tint, 0.12))
                        .child(icon(if connected { "wifi" } else { "wifi-off" }, 24., c(tint))),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .font_family(theme::HEADING)
                                .font_weight(FontWeight::BOLD)
                                .text_size(px(20.))
                                .child(if connected { format!("{name} is connected") } else { format!("can't reach {name}") }),
                        )
                        .child(div().text_size(px(13.)).text_color(c(theme::SUBTEXT0)).child(format!(
                            "{}{}",
                            synced.unwrap_or_default(),
                            if connected {
                                "new clips come over the moment they're saved."
                            } else {
                                "anything it saves waits on the frame, nothing is lost."
                            }
                        ))),
                ),
        )
        .child(card().flex().flex_col().children(rows))
        .when(trouble, |d| {
            d.child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .p(px(16.))
                    .rounded(px(14.))
                    .bg(a(theme::MAUVE, 0.06))
                    .child(icon("info", 18., c(theme::MAUVE)))
                    .child(div().flex_1().min_w_0().text_size(px(13.)).text_color(c(theme::SUBTEXT1)).child(
                        "asleep? put the headset on for a second. its wi-fi comes back when it wakes, then this finds it again by itself.",
                    )),
            )
        })
        .child(
            div()
                .flex()
                .items_center()
                .gap(px(10.))
                .child(if check.running {
                    outline("check-again", "checking").child(spinner("check-spin", 14., c(theme::MAUVE))).opacity(0.6).cursor_default()
                } else {
                    outline("check-again", "try again")
                        .child(icon("retry", 14., c(theme::TEXT)))
                        .on_click(cx.listener(|app, _, _, cx| app.run_check(cx)))
                })
                .when(!connected, |d| {
                    d.child(
                        primary("check-pair", "pair again")
                            .h(px(36.))
                            .text_size(px(13.))
                            .on_click(cx.listener(move |app, _, _, cx| app.open_pair_for(fp.clone(), cx))),
                    )
                }),
        )
        .into_any_element()
}

fn row(step: &Step, first: bool) -> AnyElement {
    let (bg, fg) = match step.outcome {
        Outcome::Pass => (a(theme::GREEN, 0.15), theme::GREEN),
        Outcome::Fail => (a(theme::RED, 0.15), theme::RED),
        _ => (a(theme::TEXT, 0.08), theme::OVERLAY2),
    };
    let mark: AnyElement = match step.outcome {
        Outcome::Pass => icon("check", 16., c(fg)).into_any_element(),
        Outcome::Fail => icon("x", 14., c(fg)).into_any_element(),
        Outcome::Running => spinner(SharedString::from(format!("step-{}", step.title)), 16., c(theme::MAUVE)).into_any_element(),
        _ => div().w(px(8.)).h(px(2.)).rounded_full().bg(c(fg)).into_any_element(),
    };
    div()
        .flex()
        .items_center()
        .gap(px(14.))
        .px(px(16.))
        .py(px(12.))
        .when(!first, |d| d.border_t_1().border_color(theme::line()))
        .child(div().size(px(32.)).flex_none().flex().items_center().justify_center().rounded_full().bg(bg).child(mark))
        .child(text(&step.title, &step.detail))
        .into_any_element()
}

fn text(head: &str, sub: &str) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .flex()
        .flex_col()
        .child(div().font_weight(FontWeight::MEDIUM).child(head.to_string()))
        .child(div().text_size(px(12.)).text_color(c(theme::SUBTEXT0)).child(sub.to_string()))
}

//! Bounded floating HUD, observation and activity callouts for the Orb only.
use crate::{
    animation::Motion,
    mesh_legend,
    mesh_model::{AgentState, Event, Simulation, ease},
    mesh_orb, mesh_stats,
    orb_hud::{Controls, Panel, Reveal},
};
use egui::{Color32, FontId, Painter, Pos2, Rect, Stroke, Ui, UiBuilder, pos2, vec2};
use herdr_mesh_visualizer::{client::View, diagnostics::json, projection::Key};
use std::collections::{BTreeMap, BTreeSet};

const MAX_PRESENTATIONS: usize = 128; // One visible page and one retracting page.
const CYAN: Color32 = Color32::from_rgb(90, 216, 235);

struct Dismissal {
    episode: Option<u64>,
    at: f64,
}
#[derive(PartialEq, Eq)]
struct FlyoutTrace {
    episode: u64,
    idle: bool,
    event_serial: Option<u64>,
    reason: &'static str,
    position: Option<usize>,
    page_start: usize,
    capacity: usize,
    candidates: usize,
    anchor: bool,
    live: bool,
}
#[derive(PartialEq, Eq)]
struct LayoutTrace {
    page_start: usize,
    capacity: usize,
    candidates: usize,
    omitted_geometry: usize,
    omitted_activities: usize,
    width: u32,
    height: u32,
    passive: bool,
}
#[derive(Default)]
pub struct Panels {
    pub page: usize,
    pub projects: bool,
    pub focus: Option<Key>,
    dismissed: BTreeMap<Key, Dismissal>,
    dismissal_revision: Option<(u64, u64, u64)>,
    pending_dismissals: Vec<(Key, u64)>,
    previous_episodes: Option<BTreeMap<Key, u64>>,
    stop_order: BTreeMap<Key, (u64, u64)>,
    stop_sequence: u64,
    controls: Controls,
    key_motion: Option<Motion>,
    count_motion: Option<Motion>,
    project_motion: Option<Motion>,
    focus_motion: Option<Motion>,
    page_start: usize,
    previous: Vec<usize>,
    membership: Vec<CardId>,
    objects: crate::orb_objects::Objects,
    presentations: BTreeMap<CardId, Presentation>,
    page_since: f64,
    diagnostic_renderer: Option<u64>,
    diagnostic_flyouts: BTreeMap<Key, FlyoutTrace>,
    diagnostic_layout: Option<LayoutTrace>,
}
impl Panels {
    pub fn reset_source(&mut self) {
        self.page = 0;
        self.page_start = 0;
        self.previous.clear();
        self.membership.clear();
        self.objects = Default::default();
        self.presentations.clear();
        self.project_motion = None;
        self.focus_motion = None;
        self.projects = false;
        self.focus = None;
        self.dismissed.clear();
        self.dismissal_revision = None;
        self.pending_dismissals.clear();
        self.previous_episodes = None;
        self.stop_order.clear();
        self.stop_sequence = 0;
        self.diagnostic_flyouts.clear();
        self.diagnostic_layout = None;
    }
    fn refresh_dismissals(&mut self, view: &View) {
        let revision = (view.epoch, view.revision, view.acknowledgement_revision);
        if self.dismissal_revision == Some(revision) {
            return;
        }
        self.dismissal_revision = Some(revision);
        self.stop_order
            .retain(|key, (episode, _)| view.stop_episode(key) == Some(*episode));
        for (key, episode) in view.stop_episodes() {
            let idle = view.stop_is_idle(key);
            if (self.previous_episodes.is_some() || idle || view.outcome_notices.is_some())
                && self.previous_episodes.as_ref().and_then(|p| p.get(key)) != Some(episode)
            {
                self.stop_sequence = self.stop_sequence.wrapping_add(1);
                self.stop_order
                    .insert(key.clone(), (*episode, self.stop_sequence));
                view.diagnostic(
                    if idle {
                        "idle_stop_priority"
                    } else {
                        "completion_priority"
                    },
                    json!({"key":view.agent_key(key), "card_key":key, "episode":episode, "arrival_order":self.stop_sequence}),
                );
            }
        }
        self.previous_episodes = Some(
            view.stop_episodes()
                .map(|(key, id)| (key.clone(), *id))
                .collect(),
        );
        for (key, episode) in view
            .acknowledged_completions
            .iter()
            .chain(view.acknowledged_idle_stops.iter())
            .chain(
                view.outcome_notices
                    .iter()
                    .flat_map(|m| m.iter())
                    .filter(|(_, n)| n.dismissed)
                    .map(|(k, n)| (k, &n.episode)),
            )
        {
            if view.stop_acknowledged(key, *episode)
                && !self
                    .dismissed
                    .get(key)
                    .is_some_and(|d| d.episode == Some(*episode))
            {
                view.diagnostic(
                    "panel_ack_imported",
                    json!({"key":view.agent_key(key), "card_key":key, "episode":episode}),
                );
                self.dismissed.insert(
                    key.clone(),
                    Dismissal {
                        episode: Some(*episode),
                        // A restored acknowledgement has already finished retracting.
                        at: f64::NEG_INFINITY,
                    },
                );
            }
        }
        if self.dismissed.is_empty() {
            return;
        }
        let mut retained = BTreeSet::new();
        fn scan(
            branch: &herdr_mesh_visualizer::projection::Branch,
            dismissed: &BTreeMap<Key, Dismissal>,
            view: &View,
            retained: &mut BTreeSet<Key>,
        ) {
            if branch.kind == "agent"
                && (branch.status == "done"
                    || branch.status == "idle" && view.idle_stop_episodes.contains_key(&branch.key))
                && dismissed
                    .get(&branch.key)
                    .is_some_and(|d| d.episode == view.stop_episode(&branch.key))
            {
                retained.insert(branch.key.clone());
            }
            for child in &branch.children {
                scan(child, dismissed, view, retained);
            }
        }
        if let Some(scene) = &view.scene {
            for node in &scene.nodes {
                scan(node, &self.dismissed, view, &mut retained);
            }
        }
        for (key, n) in view.outcome_notices.iter().flat_map(|m| m.iter()) {
            if self
                .dismissed
                .get(key)
                .is_some_and(|d| d.episode == Some(n.episode))
            {
                retained.insert(key.clone());
            }
        }
        self.dismissed.retain(|key, _| retained.contains(key));
    }
    pub fn take_dismissals(&mut self) -> Vec<(Key, u64)> {
        std::mem::take(&mut self.pending_dismissals)
    }
    pub fn handle_input(&mut self, ui: &Ui, clock: f64, passive: bool) {
        self.controls.update(clock);
        if passive {
            return;
        }
        ui.input(|input| {
            for event in &input.events {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    modifiers,
                    ..
                } = event
                {
                    if modifiers.ctrl || modifiers.alt || modifiers.command {
                        continue;
                    }
                    match key {
                        egui::Key::K => self.controls.toggle(Panel::Key, clock),
                        egui::Key::F => self.controls.toggle(Panel::Counts, clock),
                        egui::Key::N => self.objects.toggle(crate::orb_objects::Class::Node),
                        egui::Key::S => self.objects.toggle(crate::orb_objects::Class::Session),
                        egui::Key::W => self.objects.toggle(crate::orb_objects::Class::Workspace),
                        egui::Key::A => self.controls.toggle_workers(clock),
                        egui::Key::Escape => self.focus = None,
                        _ => {}
                    }
                }
            }
        });
    }
}
impl Panels {
    pub fn trace_unavailable(&mut self, sim: &Simulation, view: &View, passive: bool, rect: Rect) {
        self.trace_flyouts(
            &Context {
                sim,
                view,
                rect,
                selected: None,
                passive,
            },
            &[],
            0,
            0,
            Some("passive_unavailable"),
        );
    }
    fn trace_flyouts(
        &mut self,
        context: &Context<'_>,
        candidates: &[Card<'_>],
        page_start: usize,
        capacity: usize,
        forced: Option<&'static str>,
    ) {
        let Some(log) = &context.view.diagnostics else {
            return;
        };
        let renderer = *self.diagnostic_renderer.get_or_insert_with(|| {
            context
                .sim
                .diagnostic_renderer
                .unwrap_or_else(|| log.new_renderer())
        });
        let layout = LayoutTrace {
            page_start,
            capacity,
            candidates: candidates.len(),
            omitted_geometry: context.sim.omitted,
            omitted_activities: context.sim.omitted_activities,
            width: context.rect.width().max(0.) as u32,
            height: context.rect.height().max(0.) as u32,
            passive: context.passive,
        };
        if self.diagnostic_layout.as_ref() != Some(&layout) {
            context.view.diagnostic("callout_layout", json!({"renderer":renderer, "page_start":page_start, "capacity":capacity, "candidates":candidates.len(), "omitted_geometry":context.sim.omitted, "omitted_activities":context.sim.omitted_activities, "width_points":layout.width, "height_points":layout.height, "passive":context.passive}));
            self.diagnostic_layout = Some(layout);
        }
        let positions: BTreeMap<_, _> = candidates
            .iter()
            .enumerate()
            .filter_map(|(i, c)| c.key.filter(|_| c.acknowledgeable).map(|k| (k, i)))
            .collect();
        self.diagnostic_flyouts.retain(|key, trace| {
            if context.view.stop_episode(key) == Some(trace.episode) { return true; }
            context.view.diagnostic(if trace.idle { "idle_stop_flyout_retired" } else { "completion_flyout_retired" }, json!({"renderer":renderer, "key":context.view.agent_key(key), "card_key":key, "episode":trace.episode, "previous_reason":trace.reason}));
            false
        });
        for (key, episode) in context.view.stop_episodes() {
            let idle = context.view.stop_is_idle(key);
            let activity = context.sim.activities.get(key);
            let position = positions.get(key).copied();
            let acknowledged = self
                .dismissed
                .get(key)
                .is_some_and(|d| d.episode == Some(*episode))
                || context.view.stop_acknowledged(key, *episode);
            let reason = if let Some(reason) = forced {
                reason
            } else if acknowledged {
                if position.is_some() {
                    "acknowledgement_retracting"
                } else {
                    "acknowledged"
                }
            } else if activity.is_none() {
                "activity_missing"
            } else if activity.is_some_and(|a| {
                a.state
                    != if idle {
                        AgentState::Idle
                    } else {
                        AgentState::Completed
                    }
                    || a.stop_episode != Some(*episode)
            }) {
                "activity_episode_mismatch"
            } else if position.is_none() {
                "candidate_excluded"
            } else if capacity == 0 {
                "no_layout_space"
            } else if position.is_some_and(|i| i < page_start || i >= page_start + capacity) {
                "off_page"
            } else if position
                .is_some_and(|i| !self.presentations.contains_key(&candidates[i].id()))
            {
                "waiting_animation_capacity"
            } else if position
                .is_some_and(|i| !card_reveal(&candidates[i], context, self).interactive())
            {
                "revealing"
            } else {
                "drawn"
            };
            let trace = FlyoutTrace {
                episode: *episode,
                idle,
                event_serial: activity.map(|a| a.event.serial),
                reason,
                position,
                page_start,
                capacity,
                candidates: candidates.len(),
                anchor: context.sim.anchor(key).is_some(),
                live: context.view.live,
            };
            if self.diagnostic_flyouts.get(key) != Some(&trace) {
                context.view.diagnostic(if idle { "idle_stop_flyout" } else { "completion_flyout" }, json!({"renderer":renderer, "key":context.view.agent_key(key), "card_key":key, "episode":episode, "reason":reason, "event_serial":trace.event_serial, "activity_episode":activity.and_then(|a| a.stop_episode), "activity_state":activity.map(|a| format!("{:?}", a.state)), "acknowledged":acknowledged, "candidate_index":position, "page_start":page_start, "capacity":capacity, "candidates":candidates.len(), "anchor":trace.anchor, "live":trace.live, "omitted_activities":context.sim.omitted_activities, "previous_reason":self.diagnostic_flyouts.get(key).map(|t| t.reason)}));
                self.diagnostic_flyouts.insert(key.clone(), trace);
            }
        }
    }
}
pub struct Context<'a> {
    pub sim: &'a Simulation,
    pub view: &'a View,
    pub rect: Rect,
    pub selected: Option<&'a Key>,
    pub passive: bool,
}
#[derive(Default)]
pub struct Response {
    pub blocked: Vec<Rect>,
    pub clear_selection: bool,
}
struct Dock {
    panel: Panel,
    bounds: Rect,
    reveal: Reveal,
}
fn drift(clock: f64, phase: f64, amplitude: f32) -> f32 {
    ((clock * std::f64::consts::TAU / 48. + phase).sin() as f32) * amplitude
}

pub(crate) fn bounded(bounds: Rect, viewport: Rect) -> Rect {
    Rect::from_min_max(
        pos2(
            bounds.left().clamp(viewport.left(), viewport.right()),
            bounds.top().clamp(viewport.top(), viewport.bottom()),
        ),
        pos2(
            bounds.right().clamp(viewport.left(), viewport.right()),
            bounds.bottom().clamp(viewport.top(), viewport.bottom()),
        ),
    )
}

fn dock_layout(context: &Context<'_>, state: &mut Panels) -> Vec<Dock> {
    let clock = context.sim.clock;
    state.controls.update(clock);
    let key = state.controls.key.occupies(clock);
    let counts = state.controls.counts.occupies(clock);
    let rect = context.rect;
    let gap = 14_f32.min(rect.height() * 0.05);
    let natural: f32 = (if key { 380. } else { 0. }) + (if counts { 198. } else { 0. });
    // Reserve motion room even when both panels need compact rows.
    let drift_space = 24_f32.min(rect.height() * 0.1);
    let available = (rect.height() - drift_space - if key && counts { gap } else { 0. }).max(0.);
    let scale = (available / natural.max(1.)).min(1.);
    let kh = 380. * scale;
    let nh = 198. * scale;
    let total = (if key { kh } else { 0. })
        + (if counts { nh } else { 0. })
        + if key && counts { gap } else { 0. };
    let amplitude = 12_f32.min(((rect.height() - total) * 0.5).max(0.));
    let bottom = rect.bottom() - amplitude;
    let width = 190_f32.min(rect.width());
    let (ky, ny) = if key && counts {
        match state.controls.bottom {
            Panel::Key => (bottom - kh, bottom - kh - gap - nh),
            Panel::Counts => (bottom - nh - gap - kh, bottom - nh),
        }
    } else {
        (bottom - kh, bottom - nh)
    };
    let mut result = Vec::new();
    for (panel, shown, reveal, y, height, motion) in [
        (
            Panel::Key,
            key,
            state.controls.key.reveal(clock),
            ky,
            kh,
            &mut state.key_motion,
        ),
        (
            Panel::Counts,
            counts,
            state.controls.counts.reveal(clock),
            ny,
            nh,
            &mut state.count_motion,
        ),
    ] {
        let target = Rect::from_min_size(pos2(rect.left(), y), vec2(width, height));
        if !shown || !reveal.active() {
            *motion = Some(Motion::new(target, 1., clock));
        } else {
            motion
                .get_or_insert_with(|| Motion::new(target, 1., clock))
                .retarget(target, 1., clock);
        }
        if shown {
            let bounds = bounded(
                motion
                    .as_ref()
                    .unwrap()
                    .sample(clock)
                    .0
                    .translate(vec2(0., drift(clock, 0., amplitude))),
                rect,
            );
            result.push(Dock {
                panel,
                bounds,
                reveal,
            });
        }
    }
    // A resized viewport can invalidate intermediate geometry. Keep the stack
    // separated even while it is adapting to a much smaller window.
    if result.len() == 2 && result.iter().all(|dock| dock.reveal.active()) {
        let top = if state.controls.bottom == Panel::Key {
            1
        } else {
            0
        };
        let lower = 1 - top;
        let limit = result[lower].bounds.top() - gap;
        if result[top].bounds.bottom() > limit {
            result[top].bounds = bounded(
                result[top]
                    .bounds
                    .translate(vec2(0., limit - result[top].bounds.bottom())),
                rect,
            );
        }
    }
    result
}

pub fn frame(painter: &Painter, rect: Rect, color: Color32, opacity: f32) {
    painter.rect_filled(
        rect,
        3.,
        Color32::from_rgba_unmultiplied(4, 12, 24, 225).gamma_multiply(opacity),
    );
    painter.rect_stroke(
        rect,
        3.,
        Stroke::new(0.7, color.gamma_multiply(0.45 * opacity)),
        egui::StrokeKind::Inside,
    );
    for (a, b) in [
        (rect.left_top(), rect.left_top() + vec2(16., 0.)),
        (rect.left_top(), rect.left_top() + vec2(0., 10.)),
        (rect.right_bottom(), rect.right_bottom() - vec2(16., 0.)),
        (rect.right_bottom(), rect.right_bottom() - vec2(0., 10.)),
    ] {
        painter.line_segment([a, b], Stroke::new(1.5, color.gamma_multiply(opacity)));
    }
}
fn leader(painter: &Painter, anchor: Pos2, card: Rect, color: Color32) {
    let end = if card.center().x < anchor.x {
        card.right_center()
    } else {
        card.left_center()
    };
    painter.add(egui::Shape::line(
        vec![anchor, pos2((anchor.x + end.x) * 0.5, end.y), end],
        Stroke::new(0.8, color.gamma_multiply(0.6)),
    ));
    painter.circle_stroke(anchor, 4., Stroke::new(1., color));
}

fn detail_rect(context: &Context<'_>, reserved: &[Rect]) -> Rect {
    let rect = context.rect;
    let margin = 12_f32.min(rect.width() * 0.05).min(rect.height() * 0.05);
    let width = (rect.width() * 0.36)
        .clamp(240., 340.)
        .min((rect.width() - 2. * margin).max(1.));
    let x = rect.right() - width - margin;
    let bottom = reserved
        .iter()
        .filter(|r| x < r.right() + 12.)
        .map(|r| r.top() - 18.)
        .fold(rect.bottom() - margin, f32::min);
    let height = (bottom - rect.top() - 2. * margin).clamp(1., 340.);
    let amplitude = 8_f32.min(((bottom - rect.top() - height) * 0.5).max(0.));
    Rect::from_min_size(
        pos2(
            x,
            rect.top() + margin + amplitude + drift(context.sim.clock, 1.5, amplitude),
        ),
        vec2(width, height),
    )
}

#[derive(Clone, Copy)]
struct Card<'a> {
    event: &'a Event,
    key: Option<&'a Key>,
    persistent: bool,
    working: bool,
    attention: bool,
    acknowledgeable: bool,
    typed: Option<mesh_orb::Glyph>,
    pinned: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum CardId {
    Activity(u64),
    Object(Key),
}
impl Card<'_> {
    fn id(&self) -> CardId {
        if self.typed.is_some() {
            CardId::Object(self.key.unwrap().clone())
        } else {
            CardId::Activity(self.event.serial)
        }
    }
}
#[derive(Clone)]
struct Snapshot {
    event: Event,
    key: Option<Key>,
    persistent: bool,
    working: bool,
    attention: bool,
    acknowledgeable: bool,
    typed: Option<mesh_orb::Glyph>,
    pinned: bool,
}
impl Snapshot {
    fn from(card: &Card<'_>) -> Self {
        Self {
            event: card.event.clone(),
            key: card.key.cloned(),
            persistent: card.persistent,
            working: card.working,
            attention: card.attention,
            acknowledgeable: card.acknowledgeable,
            typed: card.typed,
            pinned: card.pinned,
        }
    }
    fn card(&self) -> Card<'_> {
        Card {
            event: &self.event,
            key: self.key.as_ref(),
            persistent: self.persistent,
            working: self.working,
            attention: self.attention,
            acknowledgeable: self.acknowledgeable,
            typed: self.typed,
            pinned: self.pinned,
        }
    }
}
struct Presentation {
    snapshot: Snapshot,
    motion: Motion,
    visibility: crate::orb_hud::Visibility,
}
fn present_cards(
    candidates: &[Card<'_>],
    slots: &[Rect],
    start: usize,
    state: &mut Panels,
    context: &Context<'_>,
) -> Vec<(Snapshot, Rect, Reveal, Option<usize>)> {
    let clock = context.sim.clock;
    let active: BTreeMap<_, _> = candidates
        .iter()
        .skip(start)
        .zip(slots)
        .enumerate()
        .map(|(i, (card, slot))| (card.id(), (i, card, *slot)))
        .collect();
    for (id, presentation) in &mut state.presentations {
        if !active.contains_key(id) {
            presentation.visibility.set(false, clock, 0.);
        }
    }
    state.presentations.retain(|_, p| {
        // Never animate stale outcome content/input after supersession.
        let obsolete = p.snapshot.acknowledgeable
            && p.snapshot.key.as_ref().is_some_and(|key| {
                context
                    .sim
                    .activities
                    .get(key)
                    .is_none_or(|a| a.event.serial != p.snapshot.event.serial)
            });
        !obsolete && p.visibility.occupies(clock)
    });
    // Admit in candidate priority order, not scoped-key order. At extreme churn,
    // wait for exits instead of dropping unread candidates or popping animations.
    let mut ordered: Vec<_> = active.iter().collect();
    ordered.sort_by_key(|(_, (index, _, _))| *index);
    for (id, (index, card, bounds)) in ordered {
        if !state.presentations.contains_key(id) && state.presentations.len() >= MAX_PRESENTATIONS {
            continue;
        }
        let presentation = state
            .presentations
            .entry(id.clone())
            .or_insert_with(|| Presentation {
                snapshot: Snapshot::from(card),
                motion: Motion::new(*bounds, 1., clock),
                visibility: if card.typed.is_some() {
                    crate::orb_hud::Visibility::hidden()
                } else {
                    crate::orb_hud::Visibility::new(1.)
                },
            });
        presentation.snapshot = Snapshot::from(card);
        presentation.motion.retarget(*bounds, 1., clock);
        presentation
            .visibility
            .set(true, clock, *index as f64 * 0.2);
    }
    // Previous page retracts while new page unfolds. Exits are visual only and never
    // own input, a pager, or acknowledgement controls.
    let mut displayed: Vec<_> = state
        .presentations
        .iter()
        .filter_map(|(id, p)| {
            let card = p.snapshot.card();
            let reveal = card_reveal(&card, context, state);
            if !reveal.active() {
                return None;
            }
            let drifted = p.motion.sample(clock).0.translate(vec2(
                0.,
                drift(clock, (p.snapshot.event.serial % 3) as f64, 8.),
            ));
            Some((
                p.snapshot.clone(),
                bounded(drifted, context.rect),
                reveal,
                active.get(id).map(|(i, _, _)| *i),
            ))
        })
        .collect();
    // Exit frames behind active cards; typed/object ordering cannot change priority.
    displayed.sort_by_key(|(_, _, _, index)| *index);
    displayed
}
fn close_object(ui: &mut Ui) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        vec2(28_f32.min(ui.available_width().max(1.)), 20.),
        egui::Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            "Close object callout",
        )
    });
    let color = CYAN.gamma_multiply(if response.hovered() || response.has_focus() {
        1.
    } else {
        0.65
    });
    let painter = ui.painter();
    for (corner, dx, dy) in [
        (rect.left_top(), 1., 1.),
        (rect.right_top(), -1., 1.),
        (rect.left_bottom(), 1., -1.),
        (rect.right_bottom(), -1., -1.),
    ] {
        painter.line_segment(
            [corner, corner + vec2(dx * 6., 0.)],
            Stroke::new(0.8, color),
        );
        painter.line_segment(
            [corner, corner + vec2(0., dy * 5.)],
            Stroke::new(0.8, color),
        );
    }
    for dy in [-1., 1.] {
        painter.line_segment(
            [
                rect.center() + vec2(-3.5, -3.5 * dy),
                rect.center() + vec2(3.5, 3.5 * dy),
            ],
            Stroke::new(1., color),
        );
    }
    response
        .on_hover_text("Close this object callout; this does not dismiss an agent outcome")
        .clicked()
}

fn cards(sim: &Simulation) -> Vec<Card<'_>> {
    let mut cards: Vec<_> = sim
        .activities
        .iter()
        .map(|(key, a)| Card {
            typed: None,
            pinned: false,
            event: &a.event,
            key: Some(key),
            persistent: a.persistent,
            working: a.persistent && a.state == AgentState::Working,
            attention: a.persistent && a.state == AgentState::Blocked,
            acknowledgeable: a.persistent
                && (a.state == AgentState::Completed || a.stop_episode.is_some()),
        })
        .collect();
    cards.extend(sim.events.iter().rev().map(|event| Card {
        typed: None,
        pinned: false,
        event,
        key: None,
        persistent: false,
        working: false,
        attention: false,
        acknowledgeable: false,
    }));
    // Short-lived transitions get first-page priority; persistent work remains pageable.
    cards.sort_by(|a, b| {
        a.persistent.cmp(&b.persistent).then_with(|| {
            if a.persistent {
                b.attention
                    .cmp(&a.attention)
                    .then_with(|| b.working.cmp(&a.working))
                    .then_with(|| a.key.cmp(&b.key))
            } else {
                b.event.serial.cmp(&a.event.serial)
            }
        })
    });
    cards
}
fn card_anchor(card: &Card<'_>, sim: &Simulation, rect: Rect) -> Option<Pos2> {
    if card.typed.is_some() {
        let key = card.key?;
        let position = if key.first().is_some_and(|k| k == "coordinator") {
            mesh_orb::coordinator(sim.motion_time())
        } else {
            mesh_orb::visible_position(sim, sim.id(key)?)
        };
        return mesh_orb::project_sim(position, sim, rect);
    }
    let id = match card.key {
        Some(key) => sim.anchor(key)?,
        None => card.event.origin,
    };
    mesh_orb::project_sim(mesh_orb::visible_position(sim, id), sim, rect)
}

fn retained(card: &Card<'_>, sim: &Simulation) -> bool {
    card.typed.is_none()
        && card.persistent
        && card.key.is_some_and(|key| sim.activity_retained(key))
}

fn title(card: &Card<'_>, sim: &Simulation) -> String {
    if retained(card, sim) {
        format!("{} · LAST KNOWN", card.event.title)
    } else {
        card.event.title.into()
    }
}
fn card_reveal(card: &Card<'_>, context: &Context<'_>, state: &Panels) -> Reveal {
    if let Some(presentation) = state.presentations.get(&card.id()) {
        let own = presentation.visibility.reveal(context.sim.clock);
        if card.typed.is_some() {
            return own;
        }
        return Reveal::new(
            own.amount()
                .min(activity_reveal(card, context, state).amount()),
        );
    }
    activity_reveal(card, context, state)
}
fn activity_reveal(card: &Card<'_>, context: &Context<'_>, state: &Panels) -> Reveal {
    let own = Reveal::event(context.sim.clock - card.event.started, card.persistent);
    if let Some(dismissal) = card.key.and_then(|key| state.dismissed.get(key)) {
        return Reveal::new(
            own.amount()
                .min((1. - (context.sim.clock - dismissal.at) / crate::orb_hud::RETRACT) as f32),
        );
    }
    if card.working {
        Reveal::new(
            own.amount()
                .min(state.controls.workers.reveal(context.sim.clock).amount()),
        )
    } else {
        own
    }
}
fn card_size(
    painter: &Painter,
    card: &Card<'_>,
    sim: &Simulation,
    limit: egui::Vec2,
    pager: bool,
) -> egui::Vec2 {
    let natural = painter
        .layout_no_wrap(card.event.text.clone(), FontId::monospace(10.), CYAN)
        .size()
        .x
        .max(
            painter
                .layout_no_wrap(title(card, sim), FontId::monospace(11.), CYAN)
                .size()
                .x,
        )
        + if card.typed.is_some() { 66. } else { 24. };
    let width = natural.max(if pager { 210. } else { 120. }).min(limit.x);
    let heading = painter.layout(
        title(card, sim),
        FontId::monospace(11.),
        CYAN,
        (width - if card.typed.is_some() { 66. } else { 24. }).max(1.),
    );
    let body = painter.layout(
        card.event.text.clone(),
        FontId::monospace(10.),
        CYAN,
        (width - 24.).max(1.),
    );
    vec2(
        width,
        (heading
            .size()
            .y
            .max(if card.typed.is_some() { 34. } else { 0. })
            + body.size().y
            + 34.
            + 24.
            + if pager { 28. } else { 0. })
        .min(limit.y),
    )
}
fn places(
    painter: &Painter,
    context: &Context<'_>,
    candidates: &[Card<'_>],
    start: usize,
    reserved: &[Rect],
    pager: bool,
) -> Vec<Rect> {
    let rect = context.rect;
    let width = 320_f32
        .min(rect.width() * 0.38)
        .max(120.)
        .min((rect.width() - 36.).max(1.));
    let height = 240_f32.min((rect.height() - 40.).max(1.));
    if width < 64. || height < 64. {
        return Vec::new();
    }
    let columns = if rect.width() >= width * 2. + 48. {
        2
    } else {
        1
    };
    let mut column = 0;
    let mut y = rect.top() + 20.;
    let mut result = Vec::new();
    for card in candidates.iter().skip(start).take(64) {
        let size = card_size(
            painter,
            card,
            context.sim,
            vec2(width, height),
            pager && result.is_empty(),
        );
        loop {
            let x = if column == 0 {
                rect.right() - size.x - 18.
            } else {
                rect.left() + 18.
            };
            let slot = Rect::from_min_size(pos2(x, y), size);
            if let Some(bottom) = reserved
                .iter()
                .filter(|r| r.expand(14.).intersects(slot))
                .map(|r| r.bottom() + 15.)
                .max_by(f32::total_cmp)
            {
                y = bottom;
                continue;
            }
            if y + size.y + 12. <= rect.bottom() {
                result.push(slot);
                y += size.y + 28.;
                break;
            }
            column += 1;
            if column >= columns {
                return result;
            }
            y = rect.top() + 20.;
        }
    }
    result
}
fn page_controls(
    ui: &mut Ui,
    state: &mut Panels,
    count: usize,
    capacity: usize,
    interactive: bool,
    clock: f64,
) {
    let width = ui.available_width();
    let caption = format!("{} callouts · page {}", count, state.page + 1);
    if interactive && capacity > 0 {
        if width < 160. {
            ui.spacing_mut().item_spacing.x = 4.;
        }
        // Reserve navigation first: a wrapping caption must not strand either button.
        if ui
            .add_enabled(!state.previous.is_empty(), egui::Button::new("‹"))
            .on_hover_text(format!("Previous · {caption}"))
            .clicked()
        {
            state.page_start = state.previous.pop().unwrap_or(0);
            state.page = state.previous.len();
            state.page_since = clock;
        }
        if ui
            .add_enabled(state.page_start + capacity < count, egui::Button::new("›"))
            .on_hover_text(format!("Next · {caption}"))
            .clicked()
        {
            state.previous.push(state.page_start);
            state.page_start += capacity;
            state.page = state.previous.len();
            state.page_since = clock;
        }
        if width >= 180. {
            ui.small(&caption);
        } else if width >= 80. {
            ui.small(format!("{}", state.page + 1))
                .on_hover_text(&caption);
        }
    } else {
        ui.small(caption);
    }
}

fn dismiss_stop(
    ui: &mut Ui,
    key: &Key,
    state: &mut Panels,
    clock: f64,
    episode: Option<u64>,
    view: &View,
) {
    let idle = view.stop_is_idle(key);
    let (rect, response) = ui.allocate_exact_size(
        vec2(28_f32.min(ui.available_width().max(1.)), 20.),
        egui::Sense::click(),
    );
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            if idle {
                "Dismiss idle work-stop notification"
            } else {
                "Dismiss completed notification"
            },
        )
    });
    if response.clicked() {
        view.diagnostic("dismiss_input", json!({"key":view.agent_key(key), "card_key":key, "episode":episode, "stop_kind":if idle { "idle" } else { "completed" }}));
        state
            .dismissed
            .insert(key.clone(), Dismissal { episode, at: clock });
        if let Some(episode) = episode {
            state.pending_dismissals.push((key.clone(), episode));
        }
    }
    let glow = ui.ctx().animate_bool_with_time(
        response.id.with("glow"),
        response.hovered() || response.has_focus(),
        0.2,
    );
    let color = if idle {
        Color32::from_rgb(155, 185, 215)
    } else {
        Color32::from_rgb(120, 235, 165)
    }
    .gamma_multiply(0.65 + 0.35 * glow);
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        2.,
        if idle {
            Color32::from_rgba_unmultiplied(12, 22, 32, (25. + 55. * glow) as u8)
        } else {
            Color32::from_rgba_unmultiplied(8, 30, 22, (25. + 55. * glow) as u8)
        },
    );
    let rail = rect.shrink(1_f32.min(rect.width() * 0.25));
    for (corner, dx, dy) in [
        (rail.left_top(), 1., 1.),
        (rail.right_top(), -1., 1.),
        (rail.left_bottom(), 1., -1.),
        (rail.right_bottom(), -1., -1.),
    ] {
        painter.line_segment(
            [
                corner + vec2(dx * 7_f32.min(rail.width().max(0.) * 0.3), 0.),
                corner,
            ],
            Stroke::new(0.8, color),
        );
        painter.line_segment(
            [corner, corner + vec2(0., dy * 5.)],
            Stroke::new(0.8, color),
        );
    }
    let radius = 3.5_f32.min((rect.width() - 6.).max(0.) * 0.5);
    for dy in [-1., 1.] {
        painter.line_segment(
            [
                rect.center() + vec2(-radius, -dy * radius),
                rect.center() + vec2(radius, dy * radius),
            ],
            Stroke::new(1.2, color),
        );
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(if idle {
            "Dismiss this work stop; a later Working → Idle stop will appear again"
        } else {
            "Dismiss this completion; a later completion will appear again"
        });
}

pub fn draw(ui: &mut Ui, context: &Context<'_>, state: &mut Panels) -> Response {
    state.refresh_dismissals(context.view);
    let rect = context.rect;
    let painter = ui.painter().with_clip_rect(rect);
    let docks = dock_layout(context, state);
    let mut response = Response::default();
    state
        .objects
        .update(context.sim, context.view, context.selected);
    let objects: Vec<_> = state.objects.cards.values().cloned().collect();
    let mut reserved: Vec<_> = docks.iter().map(|d| d.bounds).collect();
    if !context.passive
        && let Some(bounds) = crate::orb_focus::panel(
            ui,
            context.view,
            rect,
            &mut state.focus,
            &reserved,
            &mut state.focus_motion,
            context.sim.clock,
        )
    {
        reserved.push(bounds);
        response.blocked.push(bounds);
    }
    let detail = state
        .projects
        .then(|| {
            let target = detail_rect(context, &reserved);
            let motion = state
                .project_motion
                .get_or_insert_with(|| Motion::new(target, 1., context.sim.clock));
            motion.retarget(target, 1., context.sim.clock);
            bounded(motion.sample(context.sim.clock).0, rect)
        })
        .filter(|r| r.width() >= 64. && r.height() >= 64.);
    if let Some(card) = detail {
        reserved.push(card);
        response.blocked.push(card);
    }
    let mut candidates: Vec<_> = cards(context.sim)
        .into_iter()
        .filter(|c| !c.working || state.controls.workers.occupies(context.sim.clock))
        .filter(|c| {
            activity_reveal(c, context, state).active()
                || c.key.is_none_or(|key| !state.dismissed.contains_key(key))
        })
        .collect();
    candidates.retain(|c| {
        c.key
            .is_none_or(|key| !state.objects.cards.contains_key(key))
    });
    candidates.extend(objects.iter().map(|o| Card {
        event: &o.event,
        key: Some(&o.key),
        persistent: true,
        working: false,
        attention: false,
        acknowledgeable: false,
        typed: Some(o.glyph),
        pinned: context.selected == Some(&o.key),
    }));
    // A visible worker must not vanish behind still-working cards when work stops.
    // Keep new Done and observed Idle-stop instances ahead until acknowledged/state change;
    // never derive their order from sampled history/GPU admission event serials.
    candidates.sort_by_key(|card| {
        (
            std::cmp::Reverse(card.pinned),
            card.typed.is_some() && !card.pinned,
            std::cmp::Reverse(
                card.key
                    .filter(|_| card.acknowledgeable)
                    .and_then(|key| state.stop_order.get(key))
                    .map(|(_, order)| *order),
            ),
        )
    });
    if state.membership.len() != candidates.len()
        || !state
            .membership
            .iter()
            .zip(&candidates)
            .all(|(id, card)| match id {
                CardId::Object(key) => card.typed.is_some() && card.key == Some(key),
                CardId::Activity(serial) => card.typed.is_none() && card.event.serial == *serial,
            })
    {
        state.membership = candidates.iter().map(|c| c.id()).collect();
        state.page_start = 0;
        state.previous.clear();
        state.page = 0;
        state.page_since = context.sim.clock;
    }
    let mut slots = places(
        &painter,
        context,
        &candidates,
        state.page_start,
        &reserved,
        false,
    );
    if state.page_start > 0 || state.page_start + slots.len() < candidates.len() {
        slots = places(
            &painter,
            context,
            &candidates,
            state.page_start,
            &reserved,
            true,
        );
    }
    if context.passive && context.sim.clock - state.page_since >= 12. && !slots.is_empty() {
        let next = state.page_start + slots.len();
        if next < candidates.len() {
            state.page_start = next;
            state.page += 1;
        } else {
            state.page_start = 0;
            state.page = 0;
        }
        state.page_since = context.sim.clock;
        slots = places(
            &painter,
            context,
            &candidates,
            state.page_start,
            &reserved,
            false,
        );
    }
    let capacity = slots.len();
    if capacity == 0 && !candidates.is_empty() {
        painter.text(
            rect.left_top(),
            egui::Align2::LEFT_TOP,
            format!("{} callouts · enlarge view", candidates.len()),
            FontId::monospace(9.),
            CYAN,
        );
    }
    // Capture this frame's cursor: clicking a pager changes only the next frame.
    let page_start = state.page_start;
    let pager = page_start > 0 || page_start + capacity < candidates.len();
    let displayed = present_cards(&candidates, &slots, page_start, state, context);
    let waiting = candidates
        .iter()
        .skip(page_start)
        .take(capacity)
        .filter(|card| !state.presentations.contains_key(&card.id()))
        .count();
    if waiting > 0 {
        painter.text(
            rect.left_top() + vec2(0., 28.),
            egui::Align2::LEFT_TOP,
            format!("{waiting} callouts waiting for animation"),
            FontId::monospace(9.),
            CYAN,
        );
    }

    let mut label_obstacles = reserved.clone();
    label_obstacles.extend(displayed.iter().map(|(_, bounds, _, _)| bounds.expand(3.)));
    if !context.passive {
        crate::orb_focus::labels(
            ui,
            context.sim,
            context.view,
            rect,
            &state.focus,
            &label_obstacles,
        );
    }
    if let Some(anchor) = mesh_orb::project_sim(
        mesh_orb::coordinator(context.sim.motion_time()),
        context.sim,
        rect,
    ) {
        for dock in &docks {
            dock.reveal.leader(&painter, anchor, dock.bounds, CYAN);
        }
        painter.circle_stroke(
            anchor,
            14.,
            Stroke::new(1., Color32::from_rgb(242, 177, 70)),
        );
    }
    for (snapshot, bounds, reveal, _) in &displayed {
        let card = snapshot.card();
        if let Some(anchor) = card_anchor(&card, context.sim, rect) {
            let [r, g, b] = card.event.color;
            reveal.leader(
                &painter,
                anchor,
                *bounds,
                egui::Rgba::from_rgb(r, g, b).into(),
            );
        }
    }
    if let Some(card) = detail
        && let Some(anchor) = mesh_orb::project_sim(
            mesh_orb::coordinator(context.sim.motion_time()),
            context.sim,
            rect,
        )
    {
        leader(&painter, anchor, card, CYAN);
    }
    for (snapshot, bounds, reveal, index) in &displayed {
        let card = snapshot.card();
        let bounds = *bounds;
        let reveal = *reveal;
        if !reveal.active() {
            continue;
        }
        let aperture = reveal.aperture(bounds).intersect(rect);
        response.blocked.push(aperture);
        let [r, g, b] = card.event.color;
        let color: Color32 = egui::Rgba::from_rgb(r, g, b).into();
        let opacity = reveal.opacity()
            * if retained(&card, context.sim) {
                0.65
            } else {
                1.
            };
        let mut card_painter = painter.with_clip_rect(aperture);
        frame(&card_painter, aperture, color, ease(opacity + 0.25));
        card_painter.multiply_opacity(opacity);
        let header_offset = if card.typed.is_some() { 54. } else { 12. };
        let heading = card_painter.layout(
            title(&card, context.sim),
            FontId::monospace(11.),
            color,
            (bounds.width() - header_offset - 12.).max(1.),
        );
        if let Some(glyph) = card.typed {
            mesh_legend::sample(
                &card_painter,
                glyph,
                Rect::from_min_size(bounds.min + vec2(8., 6.), vec2(40., 40.)),
                context.sim.time,
            );
        }
        card_painter.galley(
            bounds.min + vec2(header_offset, 12.),
            heading.clone(),
            color,
        );
        let has_pager = *index == Some(0) && pager && !context.passive;
        let body = Rect::from_min_max(
            bounds.min
                + vec2(
                    12.,
                    heading
                        .size()
                        .y
                        .max(if card.typed.is_some() { 28. } else { 0. })
                        + 22.,
                ),
            bounds.max - vec2(12., if has_pager { 64. } else { 36. }),
        );
        let body = Rect::from_min_max(body.min.min(body.max), body.max);
        let mut child = ui.new_child(
            UiBuilder::new()
                .id(ui.id().with(("orb-activity", card.id())))
                .max_rect(body),
        );
        child.set_clip_rect(body.intersect(aperture));
        if !reveal.interactive() || index.is_none() {
            child.disable();
        }
        child.set_opacity(opacity);
        child.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        egui::ScrollArea::vertical()
            .max_height(body.height().max(1.))
            .auto_shrink([false, false])
            .show(&mut child, |ui| {
                ui.label(
                    egui::RichText::new(&card.event.text)
                        .monospace()
                        .size(10.)
                        .color(color),
                );
            });
        if !context.passive {
            let footer = Rect::from_min_max(
                pos2(
                    bounds.left() + 12.,
                    bounds.bottom() - if has_pager { 56. } else { 30. },
                ),
                pos2(
                    bounds.right() - 12.,
                    bounds.bottom() - if has_pager { 34. } else { 8. },
                ),
            );
            let mut child = ui.new_child(
                UiBuilder::new()
                    // Hierarchy/observation insertion must not change these widget IDs.
                    .id(ui.id().with(("orb-activity-focus", card.id())))
                    .max_rect(footer),
            );
            child.set_clip_rect(footer.intersect(aperture));
            if !reveal.interactive() || index.is_none() {
                child.disable();
            }
            child.set_opacity(opacity);
            child.horizontal(|ui| {
                if let Some(key) = card.key.or_else(|| context.sim.key(card.event.origin)) {
                    let acknowledgeable = card.acknowledgeable;
                    let compact = ui.available_width() < 100.;
                    if acknowledgeable && compact {
                        dismiss_stop(
                            ui,
                            key,
                            state,
                            context.sim.clock,
                            context.view.stop_episode(key),
                            context.view,
                        );
                    }
                    // Removed activity keys do not inherit a recycled geometry slot.
                    if card_anchor(&card, context.sim, rect).is_some() {
                        crate::orb_focus::checkbox(
                            ui,
                            context.view,
                            if card.typed.is_some() {
                                key
                            } else {
                                context.view.agent_key(key)
                            },
                            &mut state.focus,
                        );
                    }
                    if card.typed.is_some() && close_object(ui) {
                        state.objects.close(key);
                        if context.selected == Some(key) {
                            response.clear_selection = true;
                        }
                    }
                    if acknowledgeable && !compact {
                        dismiss_stop(
                            ui,
                            key,
                            state,
                            context.sim.clock,
                            context.view.stop_episode(key),
                            context.view,
                        );
                    }
                }
            });
        }
        if has_pager {
            let footer = Rect::from_min_max(
                pos2(bounds.left() + 12., bounds.bottom() - 32.),
                bounds.max - vec2(12., 6.),
            );
            let mut child = ui.new_child(
                UiBuilder::new()
                    .id(ui.id().with("orb-activity-page"))
                    .max_rect(footer),
            );
            child.set_clip_rect(footer.intersect(aperture));
            if !reveal.interactive() {
                child.disable();
            }
            child.set_opacity(opacity);
            child.horizontal(|ui| {
                page_controls(
                    ui,
                    state,
                    candidates.len(),
                    capacity,
                    true,
                    context.sim.clock,
                )
            });
        }
        reveal.scan(&painter, bounds);
    }

    state.trace_flyouts(context, &candidates, page_start, capacity, None);

    if let Some(card) = detail {
        frame(&painter, card, CYAN, 1.);
        let body = card.shrink(12.).intersect(rect);
        let mut child = ui.new_child(
            UiBuilder::new()
                .id(ui.id().with("orb-observation-callout"))
                .max_rect(body),
        );
        child.set_clip_rect(body);
        child.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        child.horizontal(|ui| {
            ui.strong("Projects");
            if !context.passive && ui.button("Close").clicked() {
                response.clear_selection = true;
                state.projects = false;
            }
        });
        egui::ScrollArea::vertical()
            .max_height(child.available_height())
            .auto_shrink([false, false])
            .show(&mut child, |ui| {
                if let Some(scene) = &context.view.scene {
                    for project in &scene.projects {
                        ui.label(format!(
                            "{} · {} workspace observations · {} nodes",
                            project.id, project.workspaces, project.nodes
                        ));
                    }
                    if scene.projects.is_empty() {
                        ui.label("No reported project bindings");
                    }
                    ui.small("Exact reported project IDs; no Git equivalence inferred.");
                }
            });
    }

    for dock in &docks {
        let reveal = dock.reveal;
        if !reveal.active() || !dock.bounds.is_positive() {
            continue;
        }
        let aperture = reveal.aperture(dock.bounds).intersect(rect);
        response.blocked.push(aperture);
        let mut panel_painter = painter.with_clip_rect(aperture);
        frame(
            &panel_painter,
            aperture,
            CYAN,
            ease(reveal.opacity() + 0.25),
        );
        panel_painter.multiply_opacity(reveal.opacity());
        let compact = dock.bounds.height() < 100.;
        let header_height = if compact { 0. } else { 42. };
        if !compact {
            let header = Rect::from_min_max(
                dock.bounds.min + vec2(12., 6.),
                pos2(
                    dock.bounds.right() - 12.,
                    (dock.bounds.top() + header_height).min(dock.bounds.bottom()),
                ),
            );
            let mut child = ui.new_child(
                UiBuilder::new()
                    .id(ui.id().with(if dock.panel == Panel::Key {
                        "orb-key-controls"
                    } else {
                        "orb-count-controls"
                    }))
                    .max_rect(header),
            );
            child.set_clip_rect(header.intersect(aperture));
            if !reveal.interactive() {
                child.disable();
            }
            child.set_opacity(reveal.opacity());
            child.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
            child.label(
                egui::RichText::new(if context.view.live {
                    "LIVE · all known observations"
                } else {
                    "LAST KNOWN · fleet unavailable"
                })
                .monospace()
                .size(9.)
                .color(CYAN),
            );
            if dock.panel == Panel::Key {
                child.horizontal(|ui| {
                    ui.small("KEY · K").on_hover_text(
                        "N nodes · S sessions · W workspaces · F fleet · A working agents",
                    );
                    if !context.passive && ui.selectable_label(state.projects, "Projects").clicked()
                    {
                        state.projects = !state.projects;
                        response.clear_selection = true;
                    }
                });
            } else {
                child.small("FLEET · F");
            }
        }
        let body = Rect::from_min_max(
            pos2(
                dock.bounds.left() + if compact { 4. } else { 12. },
                (dock.bounds.top() + header_height).min(dock.bounds.bottom()),
            ),
            dock.bounds.max - if compact { vec2(4., 1.) } else { vec2(12., 8.) },
        );
        if body.is_positive() {
            if dock.panel == Panel::Key {
                let key = mesh_legend::layout(&panel_painter, body, body.bottom());
                mesh_legend::draw(&panel_painter, &key, context.sim.time);
            } else {
                let stats = mesh_stats::layout(&panel_painter, body, context.sim.summary());
                mesh_stats::draw(&panel_painter, &stats);
            }
        }
        reveal.scan(&painter, dock.bounds);
    }
    // Sampling is relevant even when K/F are hidden; never disguise partial detail.
    if context.sim.omitted > 0 {
        painter.text(
            rect.left_top(),
            egui::Align2::LEFT_TOP,
            format!(
                "Detail sampled: {} omitted; totals complete",
                context.sim.omitted
            ),
            egui::FontId::monospace(9.),
            CYAN,
        );
    }
    if context.sim.omitted_activities > 0 {
        painter.text(
            rect.left_top() + vec2(0., 14.),
            egui::Align2::LEFT_TOP,
            format!(
                "Callout capacity: {} omitted; node attention and totals complete",
                context.sim.omitted_activities
            ),
            FontId::monospace(9.),
            CYAN,
        );
    }
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh_model::tests::{node, view};
    use herdr_mesh_visualizer::heartbeat::Stamp;
    fn key(key: egui::Key, pressed: bool, repeat: bool) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat,
            modifiers: Default::default(),
        }
    }
    #[test]
    fn actual_keyboard_events_ignore_repeat_release_passive_and_preserve_source_preferences() {
        let ctx = egui::Context::default();
        let mut state = Panels::default();
        let run = |events, passive, time, state: &mut Panels| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| state.handle_input(ui, time, passive),
            );
            output.textures_delta.clear();
        };
        run(vec![key(egui::Key::A, true, false)], false, 2., &mut state);
        assert!(!state.controls.workers.shown);
        run(
            vec![
                key(egui::Key::A, true, true),
                key(egui::Key::A, false, false),
            ],
            false,
            3.,
            &mut state,
        );
        assert!(!state.controls.workers.shown);
        run(vec![key(egui::Key::A, true, false)], true, 4., &mut state);
        assert!(!state.controls.workers.shown);
        run(
            vec![
                key(egui::Key::K, true, false),
                key(egui::Key::F, true, false),
            ],
            false,
            5.,
            &mut state,
        );
        state.reset_source();
        run(
            vec![key(egui::Key::A, false, false)],
            false,
            200.,
            &mut state,
        );
        assert!(!state.controls.key.reveal(200.).active());
        assert!(!state.controls.counts.reveal(200.).active());
        assert!(!state.controls.workers.reveal(200.).active());
        run(
            vec![key(egui::Key::A, true, false)],
            false,
            201.,
            &mut state,
        );
        assert!(state.controls.workers.reveal(203.).interactive());
    }
    #[test]
    fn dock_is_bounded_separate_and_remaining_panel_moves_to_bottom() {
        let v = view(vec![node("one", "working", 1)], 1);
        let mut sim = Simulation::default();
        sim.update(&v, Stamp::seconds(201), 0.);
        for size in [vec2(180., 90.), vec2(320., 600.), vec2(1100., 800.)] {
            let rect = Rect::from_min_size(pos2(16., 16.), size);
            let mut state = Panels::default();
            for step in 0..1500 {
                let clock = step as f64 * 0.1;
                sim.clock = clock;
                // Hide, reopen, reverse in flight, then open from fully hidden.
                if [650, 900, 910, 915, 1100].contains(&step) {
                    state.controls.toggle(Panel::Key, clock);
                }
                if [950, 970, 1000].contains(&step) {
                    state.controls.toggle(Panel::Counts, clock);
                }
                let context = Context {
                    sim: &sim,
                    view: &v,
                    rect,
                    selected: None,
                    passive: false,
                };
                let docks = dock_layout(&context, &mut state);
                for dock in &docks {
                    assert!(
                        rect.contains_rect(dock.bounds),
                        "{size:?} {clock} {:?}",
                        dock.bounds
                    );
                }
                if docks.len() == 2 && docks.iter().all(|d| d.reveal.active()) {
                    assert!(
                        !docks[0]
                            .reveal
                            .aperture(docks[0].bounds)
                            .intersects(docks[1].reveal.aperture(docks[1].bounds)),
                        "{size:?} {clock}"
                    );
                }
                if clock > 62. && clock < 64. {
                    assert_eq!(docks.len(), 1);
                    assert!(rect.bottom() - docks[0].bounds.bottom() <= 24.01);
                }
            }
        }
    }
    #[test]
    fn abrupt_resize_keeps_animated_docks_valid_inside_the_viewport() {
        let v = view(vec![node("one", "working", 1)], 1);
        let mut sim = Simulation::default();
        sim.update(&v, Stamp::seconds(201), 2.);
        let mut state = Panels::default();
        for (time, size) in [
            (2., vec2(1100., 800.)),
            (2.1, vec2(180., 90.)),
            (2.2, vec2(180., 90.)),
            (2.6, vec2(180., 90.)),
            (3., vec2(1100., 800.)),
            (3.1, vec2(1100., 800.)),
            (3.6, vec2(1100., 800.)),
        ] {
            sim.clock = time;
            let rect = Rect::from_min_size(pos2(16., 16.), size);
            let docks = dock_layout(
                &Context {
                    sim: &sim,
                    view: &v,
                    rect,
                    selected: None,
                    passive: false,
                },
                &mut state,
            );
            for dock in &docks {
                assert!(dock.bounds.is_finite());
                assert!(dock.bounds.width() >= 0. && dock.bounds.height() >= 0.);
                assert!(
                    rect.contains_rect(dock.bounds),
                    "{time} {size:?} {:?}",
                    dock.bounds
                );
            }
            if docks.len() == 2 && docks.iter().all(|d| d.bounds.is_positive()) {
                assert!(!docks[0].bounds.intersects(docks[1].bounds));
            }
        }
    }
    #[test]
    fn w_hides_only_working_cards_while_blocked_attention_and_focus_remain_available() {
        let ctx = egui::Context::default();
        let v = view(
            vec![node("attention", "blocked", 2), node("busy", "working", 1)],
            1,
        );
        let mut sim = Simulation::default();
        sim.update(&v, Stamp::seconds(201), 0.);
        let mut state = Panels::default();
        state.controls.toggle_workers(2.);
        state.focus = Some(v.scene.as_ref().unwrap().nodes[1].key.clone());
        sim.update(&v, Stamp::seconds(201), 100.);
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(1100., 800.));
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect),
                ..Default::default()
            },
            |ui| {
                draw(
                    ui,
                    &Context {
                        sim: &sim,
                        view: &v,
                        rect,
                        selected: None,
                        passive: false,
                    },
                    &mut state,
                );
            },
        );
        assert!(output.shapes.iter().any(|s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.text().contains("ATTENTION REQUIRED"))));
        assert!(!output.shapes.iter().any(
            |s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.text().contains("AGENT WORKING"))
        ));
        assert!(output.shapes.iter().any(|s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.text().contains("Return to fleet"))));
        assert_eq!(
            sim.activities
                .values()
                .filter(|a| a.state == AgentState::Blocked)
                .count(),
            4
        );
        output.textures_delta.clear();
        let mut output = ctx.run_ui(
            egui::RawInput {
                events: vec![key(egui::Key::Escape, true, false)],
                ..Default::default()
            },
            |ui| state.handle_input(ui, 100., false),
        );
        assert!(state.focus.is_none());
        output.textures_delta.clear();
    }
    #[test]
    fn startup_key_and_callouts_cannot_hide_focused_names_or_hierarchy() {
        let ctx = egui::Context::default();
        let v = view(vec![node("one", "done", 2)], 1);
        let mut sim = Simulation::default();
        let mut state = Panels {
            focus: Some(v.scene.as_ref().unwrap().nodes[0].key.clone()),
            ..Default::default()
        };
        let mut controller = crate::orb_focus::Controller::default();
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(1100., 600.));
        sim.update(&v, Stamp::seconds(201), 0.);
        controller.update(&mut sim, &mut state.focus, rect);
        for clock in [2., 4., 30., 59.9, 60.5, 62.] {
            sim.update(&v, Stamp::seconds(201), clock);
            controller.update(&mut sim, &mut state.focus, rect);
            let names: BTreeSet<_> = sim
                .entities
                .keys()
                .map(|id| sim.name(*id).to_owned())
                .collect();
            let mut blocked = Vec::new();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    ..Default::default()
                },
                |ui| {
                    blocked = draw(
                        ui,
                        &Context {
                            sim: &sim,
                            view: &v,
                            rect,
                            selected: None,
                            passive: false,
                        },
                        &mut state,
                    )
                    .blocked;
                },
            );
            assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().contains("Rotation paused"))), "hierarchy missing at {clock}");
            let projected: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) if names.contains(t.galley.text()) => {
                        Some(Rect::from_min_size(t.pos, t.galley.size()))
                    }
                    _ => None,
                })
                .collect();
            assert!(!projected.is_empty(), "names missing at {clock}");
            assert!(
                projected.iter().all(|label| rect.contains_rect(*label)
                    && blocked.iter().all(|r| !r.intersects(*label))),
                "projected name behind overlay at {clock}"
            );
            output.textures_delta.clear();
        }
    }
    #[test]
    fn keyboard_focus_survives_the_hierarchy_appearing_before_activity_controls() {
        let ctx = egui::Context::default();
        let v = view(vec![node("one", "done", 1)], 1);
        let mut sim = Simulation::default();
        let mut state = Panels::default();
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(1100., 600.));
        sim.update(&v, Stamp::seconds(201), 0.);
        sim.update(&v, Stamp::seconds(201), 2.);
        let run = |state: &mut Panels, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    events,
                    ..Default::default()
                },
                |ui| {
                    draw(
                        ui,
                        &Context {
                            sim: &sim,
                            view: &v,
                            rect,
                            selected: None,
                            passive: false,
                        },
                        state,
                    );
                },
            );
            output.textures_delta.clear();
        };
        run(&mut state, vec![]);
        for _ in 0..12 {
            run(&mut state, vec![key(egui::Key::Tab, true, false)]);
            run(&mut state, vec![key(egui::Key::Tab, false, false)]);
            run(&mut state, vec![key(egui::Key::Space, true, false)]);
            run(&mut state, vec![key(egui::Key::Space, false, false)]);
            if state.focus.is_some() {
                break;
            }
        }
        assert!(state.focus.is_some(), "Tab must reach a reticle");
        let focused = ctx.memory(|m| m.focused());
        assert!(
            focused.is_some(),
            "reticle must retain keyboard focus after hierarchy appears"
        );
        run(&mut state, vec![]);
        assert_eq!(ctx.memory(|m| m.focused()), focused);
        run(&mut state, vec![key(egui::Key::Enter, true, false)]);
        assert!(
            state.focus.is_none(),
            "Enter must still toggle the same reticle off"
        );
        run(&mut state, vec![key(egui::Key::Enter, false, false)]);
        run(&mut state, vec![key(egui::Key::Space, true, false)]);
        run(&mut state, vec![key(egui::Key::Space, false, false)]);
        assert!(state.focus.is_some());
        run(&mut state, vec![key(egui::Key::Tab, true, false)]);
        run(&mut state, vec![key(egui::Key::Tab, false, false)]);
        run(&mut state, vec![key(egui::Key::Enter, true, false)]);
        assert_eq!(
            state.dismissed.len(),
            1,
            "Tab must reach Dismiss beside the active reticle"
        );
        assert!(
            state.focus.is_some(),
            "acknowledgement must leave Focus active"
        );
    }
    #[test]
    fn latest_outcome_card_survives_live_idle_and_work_then_replaces_and_dismisses_by_instance() {
        use herdr_mesh_visualizer::outcomes::Outcome;
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(1100., 600.));
        let mut n = node("one", "working", 12);
        n.herdr.as_mut().unwrap().agents[0].agent_status = "idle".into();
        let mut v = view(vec![n.clone()], 1);
        let scene = v.scene.as_ref().unwrap();
        let agent = scene.nodes[0]
            .children
            .iter()
            .flat_map(|s| &s.children)
            .flat_map(|w| &w.children)
            .find(|a| a.status == "idle")
            .unwrap()
            .key
            .clone();
        let mut notice = Outcome {
            source: scene.coordinator.as_ref().unwrap().key.clone(),
            agent: agent.clone(),
            episode: 10,
            kind: "done".into(),
            names: vec![
                "Actual node one".into(),
                "default".into(),
                "Actual workspace".into(),
                "Actual agent 0".into(),
            ],
            dismissed: false,
            observed_seconds: 200,
            observed_nanos: 0,
        };
        let card = notice.card_key();
        v.outcome_notices = Some(std::sync::Arc::new(BTreeMap::from([(
            card.clone(),
            notice.clone(),
        )])));
        let mut sim = Simulation::default();
        let mut state = Panels::default();
        let frame = |sim: &Simulation, v: &View, state: &mut Panels, events| {
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    events,
                    ..Default::default()
                },
                |ui| {
                    state.handle_input(ui, sim.clock, false);
                    draw(
                        ui,
                        &Context {
                            sim,
                            view: v,
                            rect,
                            selected: None,
                            passive: false,
                        },
                        state,
                    );
                },
            );
            let texts = out
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_owned()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            out.textures_delta.clear();
            texts
        };
        sim.update(&v, Stamp::seconds(201), 0.);
        for clock in [2., 30., 90., 300.] {
            sim.update(&v, Stamp::seconds(201), clock);
            let texts = frame(&sim, &v, &mut state, vec![]);
            assert!(texts.iter().any(|t| t == "AGENT COMPLETED"), "{texts:?}");
            assert_eq!(sim.state(sim.id(&agent).unwrap()), AgentState::Idle);
            assert_eq!(sim.summary().states[2], 0);
            assert!(
                sim.activities[&card]
                    .event
                    .text
                    .contains("Current state: idle")
            );
        }
        assert!(
            frame(&sim, &v, &mut state, vec![key(egui::Key::A, true, false)])
                .iter()
                .any(|t| t == "AGENT COMPLETED")
        );
        frame(&sim, &v, &mut state, vec![key(egui::Key::A, false, false)]);
        // Focus resolves the real agent, not its independent outcome card key.
        for _ in 0..20 {
            frame(&sim, &v, &mut state, vec![key(egui::Key::Tab, true, false)]);
            frame(
                &sim,
                &v,
                &mut state,
                vec![key(egui::Key::Tab, false, false)],
            );
            frame(
                &sim,
                &v,
                &mut state,
                vec![key(egui::Key::Enter, true, false)],
            );
            frame(
                &sim,
                &v,
                &mut state,
                vec![key(egui::Key::Enter, false, false)],
            );
            if state.focus.is_some() {
                break;
            }
        }
        assert_eq!(state.focus.as_ref(), Some(&agent[..2].to_vec()));
        // Working does not delete the unread result; a live work card can coexist.
        n.herdr.as_mut().unwrap().agents[0].agent_status = "working".into();
        v.scene = view(vec![n.clone()], 2).scene;
        v.revision += 1;
        sim.update(&v, Stamp::seconds(201), 301.);
        assert_eq!(sim.activities[&card].stop_episode, Some(10));
        assert!(sim.activities.contains_key(&agent));
        assert!(
            sim.activities[&card]
                .event
                .text
                .contains("Current state: working")
        );
        // A later direct Working -> Idle supersedes the previous unread Done.
        n.herdr.as_mut().unwrap().agents[0].agent_status = "idle".into();
        v.scene = view(vec![n.clone()], 3).scene;
        v.revision += 1;
        notice.episode = 11;
        notice.kind = "idle".into();
        v.outcome_notices = Some(std::sync::Arc::new(BTreeMap::from([(
            card.clone(),
            notice.clone(),
        )])));
        sim.update(&v, Stamp::seconds(201), 302.);
        sim.update(&v, Stamp::seconds(201), 304.);
        let texts = frame(&sim, &v, &mut state, vec![]);
        assert!(
            texts.iter().any(|t| t == "WORK STOPPED — IDLE"),
            "{texts:?}"
        );
        assert!(!texts.iter().any(|t| t == "AGENT COMPLETED"));
        assert_eq!(sim.activities[&card].stop_episode, Some(11));
        assert_eq!(
            sim.activities
                .keys()
                .filter(|k| k.last().unwrap() == "outcome")
                .count(),
            1
        );
        for _ in 0..20 {
            frame(&sim, &v, &mut state, vec![key(egui::Key::Tab, true, false)]);
            frame(
                &sim,
                &v,
                &mut state,
                vec![key(egui::Key::Tab, false, false)],
            );
            frame(
                &sim,
                &v,
                &mut state,
                vec![key(egui::Key::Enter, true, false)],
            );
            frame(
                &sim,
                &v,
                &mut state,
                vec![key(egui::Key::Enter, false, false)],
            );
            if state
                .dismissed
                .get(&card)
                .is_some_and(|d| d.episode == Some(11))
            {
                break;
            }
        }
        assert_eq!(state.take_dismissals(), vec![(card.clone(), 11)]);
        notice.dismissed = true;
        v.outcome_notices = Some(std::sync::Arc::new(BTreeMap::from([(
            card.clone(),
            notice.clone(),
        )])));
        v.acknowledgement_revision += 1;
        sim.update(&v, Stamp::seconds(201), 304.4);
        assert!(
            frame(&sim, &v, &mut state, vec![])
                .iter()
                .any(|t| t == "WORK STOPPED — IDLE")
        );
        sim.update(&v, Stamp::seconds(201), 306.);
        assert!(
            !frame(&sim, &v, &mut state, vec![])
                .iter()
                .any(|t| t == "WORK STOPPED — IDLE")
        );
        // A fresh Done renews the same card slot with a fresh, unacknowledged ID.
        notice.episode = 12;
        notice.kind = "done".into();
        notice.dismissed = false;
        v.outcome_notices = Some(std::sync::Arc::new(BTreeMap::from([(
            card.clone(),
            notice,
        )])));
        v.revision += 1;
        sim.update(&v, Stamp::seconds(201), 307.);
        sim.update(&v, Stamp::seconds(201), 309.);
        assert!(
            frame(&sim, &v, &mut state, vec![])
                .iter()
                .any(|t| t == "AGENT COMPLETED")
        );
        assert!(!state.dismissed.contains_key(&card));
        // Disappearance retains the result with truthful missing-agent context.
        v.scene = view(vec![], 5).scene;
        v.revision += 1;
        sim.update(&v, Stamp::seconds(201), 310.);
        assert!(
            sim.activities[&card]
                .event
                .text
                .contains("no longer observed")
        );
        assert!(sim.anchor(&card).is_none());
        assert!(
            frame(&sim, &v, &mut state, vec![])
                .iter()
                .any(|t| t.contains("AGENT COMPLETED"))
        );
        assert_eq!(sim.summary().agents, 0);
    }
    #[test]
    fn idle_work_stop_is_persistent_visible_and_independently_dismissible_on_a_crowded_first_frame()
    {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(1100., 600.));
        let mut n = node("one", "working", 12);
        n.herdr.as_mut().unwrap().agents[0].agent_status = "idle".into();
        let mut v = view(vec![n], 1);
        let dir = tempfile::tempdir().unwrap();
        let log = herdr_mesh_visualizer::diagnostics::Diagnostics::at(dir.path(), 8790).unwrap();
        v.diagnostics = Some(log.clone());
        let stop_key = v.scene.as_ref().unwrap().nodes[0]
            .children
            .iter()
            .flat_map(|s| &s.children)
            .flat_map(|w| &w.children)
            .find(|a| a.status == "idle")
            .unwrap()
            .key
            .clone();
        v.idle_stop_episodes = std::sync::Arc::new(BTreeMap::from([(stop_key.clone(), 7)]));
        let mut sim = Simulation::default();
        let mut state = Panels::default();
        sim.update(&v, Stamp::seconds(201), 0.);
        let frame = |sim: &Simulation, v: &View, state: &mut Panels, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    events,
                    ..Default::default()
                },
                |ui| {
                    state.handle_input(ui, sim.clock, false);
                    draw(
                        ui,
                        &Context {
                            sim,
                            view: v,
                            rect,
                            selected: None,
                            passive: false,
                        },
                        state,
                    );
                },
            );
            let texts: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                    _ => None,
                })
                .collect();
            output.textures_delta.clear();
            texts
        };
        for clock in [2., 30., 90., 300.] {
            sim.update(&v, Stamp::seconds(201), clock);
            let texts = frame(&sim, &v, &mut state, vec![]);
            assert!(
                texts.iter().any(|s| s == "WORK STOPPED — IDLE"),
                "clock {clock}: {texts:?}"
            );
            assert_eq!(sim.summary().states[2], 0);
            assert!(sim.activities[&stop_key].persistent);
            assert_eq!(sim.activities[&stop_key].state, AgentState::Idle);
        }
        // W hides work, not the retained Idle stop.
        assert!(
            frame(&sim, &v, &mut state, vec![key(egui::Key::A, true, false)])
                .iter()
                .any(|s| s == "WORK STOPPED — IDLE")
        );
        frame(&sim, &v, &mut state, vec![key(egui::Key::A, false, false)]);
        for _ in 0..20 {
            frame(&sim, &v, &mut state, vec![key(egui::Key::Tab, true, false)]);
            frame(
                &sim,
                &v,
                &mut state,
                vec![key(egui::Key::Tab, false, false)],
            );
            frame(
                &sim,
                &v,
                &mut state,
                vec![key(egui::Key::Enter, true, false)],
            );
            frame(
                &sim,
                &v,
                &mut state,
                vec![key(egui::Key::Enter, false, false)],
            );
            if state.dismissed.contains_key(&stop_key) {
                break;
            }
        }
        assert!(
            state.dismissed.contains_key(&stop_key),
            "keyboard must reach the Idle ×"
        );
        assert_eq!(state.take_dismissals(), vec![(stop_key.clone(), 7)]);
        std::sync::Arc::make_mut(&mut v.acknowledged_idle_stops).insert(stop_key.clone(), 7);
        v.acknowledgement_revision += 1;
        sim.update(&v, Stamp::seconds(201), 300.4);
        assert!(
            frame(&sim, &v, &mut state, vec![])
                .iter()
                .any(|s| s == "WORK STOPPED — IDLE"),
            "receiver acknowledgement must not skip retraction"
        );
        sim.update(&v, Stamp::seconds(201), 302.);
        assert!(
            !frame(&sim, &v, &mut state, vec![])
                .iter()
                .any(|s| s == "WORK STOPPED — IDLE")
        );
        assert_eq!(sim.state(sim.id(&stop_key).unwrap()), AgentState::Idle);
        // A new receiver-issued cycle renews the banner even if no Working frame was drawn.
        std::sync::Arc::make_mut(&mut v.idle_stop_episodes).insert(stop_key.clone(), 8);
        v.acknowledged_idle_stops = Default::default();
        v.revision += 1;
        sim.update(&v, Stamp::seconds(201), 303.);
        sim.update(&v, Stamp::seconds(201), 305.);
        assert!(
            frame(&sim, &v, &mut state, vec![])
                .iter()
                .any(|s| s == "WORK STOPPED — IDLE")
        );
        assert!(state.dismissed.is_empty());
        // Removing the observation-derived token leaves ordinary Idle, never synthetic Done.
        v.idle_stop_episodes = Default::default();
        v.revision += 1;
        sim.update(&v, Stamp::seconds(201), 306.);
        assert!(!sim.activities.contains_key(&stop_key));
        frame(&sim, &v, &mut state, vec![]);
        log.flush();
        let records: Vec<serde_json::Value> = std::fs::read_to_string(log.path())
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        for (event, reason) in [
            ("idle_stop_flyout", Some("drawn")),
            ("idle_stop_flyout", Some("acknowledgement_retracting")),
            ("idle_stop_flyout", Some("acknowledged")),
            ("idle_stop_flyout_retired", None),
        ] {
            assert!(records.iter().any(|r| r["event"] == event
                && reason.is_none_or(|s| r["data"]["reason"] == s)), "{event} {reason:?}");
        }
        assert!(records.iter().any(|r| r["event"] == "dismiss_input"
            && r["data"]["stop_kind"] == "idle"
            && r["data"]["episode"] == "7"));
        assert!(!records.iter().any(|r| r["event"] == "completion_flyout"));
    }
    #[test]
    fn a_visible_worker_completion_is_shown_on_the_first_page_without_dismiss_input() {
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(1100., 600.));
        let mut work = node("one", "working", 4);
        work.herdr.as_mut().unwrap().agents[0].display_name = "Freshly completed agent".into();
        let mut initial = view(vec![work.clone(), node("older", "done", 1)], 1);
        let mut sim = Simulation::default();
        sim.update(&initial, Stamp::seconds(201), 0.);
        initial.completion_episodes = std::sync::Arc::new(
            sim.activities
                .iter()
                .filter(|(_, a)| a.state == AgentState::Completed)
                .enumerate()
                .map(|(i, (key, _))| (key.clone(), i as u64 + 1))
                .collect(),
        );
        sim = Simulation::default();
        sim.update(&initial, Stamp::seconds(201), 0.);
        sim.update(&initial, Stamp::seconds(201), 2.);
        let mut state = Panels::default();
        let frame = |sim: &Simulation, view: &View, state: &mut Panels| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    ..Default::default()
                },
                |ui| {
                    draw(
                        ui,
                        &Context {
                            sim,
                            view,
                            rect,
                            selected: None,
                            passive: false,
                        },
                        state,
                    );
                },
            );
            let texts: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) => Some(t.galley.text().to_string()),
                    _ => None,
                })
                .collect();
            output.textures_delta.clear();
            texts
        };
        assert!(
            frame(&sim, &initial, &mut state)
                .iter()
                .any(|t| t.contains("Freshly completed agent"))
        );
        assert!(state.dismissed.is_empty());
        work.herdr.as_mut().unwrap().agents[0].agent_status = "done".into();
        let mut completed = view(vec![work, node("older", "done", 1)], 2);
        completed.completion_episodes = initial.completion_episodes.clone();
        let key = sim
            .activities
            .iter()
            .find(|(_, a)| a.event.text.contains("Freshly completed agent"))
            .unwrap()
            .0
            .clone();
        std::sync::Arc::make_mut(&mut completed.completion_episodes).insert(key.clone(), 99);
        sim.update(&completed, Stamp::seconds(202), 3.);
        sim.update(&completed, Stamp::seconds(202), 5.);
        let texts = frame(&sim, &completed, &mut state);
        assert!(
            texts.iter().any(|t| t == "AGENT COMPLETED"),
            "the new completion must have a visible banner"
        );
        assert!(
            texts.iter().any(|t| t.contains("Freshly completed agent")),
            "the previously visible worker must not silently move to a later page"
        );
        assert!(state.dismissed.is_empty(), "no dismissal was requested");
        assert!(state.take_dismissals().is_empty());
        assert_eq!(sim.summary().states[2], 3);
        assert!(
            state
                .membership
                .contains(&CardId::Activity(sim.activities[&key].event.serial))
        );
        // Priority has no ten-second expiry and unchanged reconnects cannot replay it.
        let order = state.stop_order[&key];
        completed.epoch += 1;
        completed.revision += 1;
        sim.update(&completed, Stamp::seconds(202), 70.);
        let texts = frame(&sim, &completed, &mut state);
        assert!(texts.iter().any(|t| t.contains("Freshly completed agent")));
        assert_eq!(state.stop_order[&key], order);
        // Geometry/history readmission serials cannot take priority from this instance.
        let old_key = initial.completion_episodes.keys().next().unwrap();
        sim.activities.get_mut(old_key).unwrap().event.serial = 999_999;
        sim.update(&completed, Stamp::seconds(202), 71.);
        let texts = frame(&sim, &completed, &mut state);
        assert!(texts.iter().any(|t| t.contains("Freshly completed agent")));
        assert_eq!(state.stop_order[&key], order);
        assert!(!state.stop_order.contains_key(old_key));
        // Retraction still needs an explicit independent acknowledgement.
        state.dismissed.insert(
            key.clone(),
            Dismissal {
                episode: Some(99),
                at: 71.,
            },
        );
        sim.update(&completed, Stamp::seconds(202), 73.);
        assert!(
            !frame(&sim, &completed, &mut state)
                .iter()
                .any(|t| t.contains("Freshly completed agent"))
        );
        assert_eq!(sim.summary().states[2], 3);
    }
    #[test]
    fn completion_diagnostics_explain_visibility_and_do_not_log_every_frame() {
        use herdr_mesh_visualizer::diagnostics::{Diagnostics, Value};
        let dir = tempfile::tempdir().unwrap();
        let log = Diagnostics::at(dir.path(), 8790).unwrap();
        let mut v = view(vec![node("one", "done", 6)], 1);
        let mut sim = Simulation::default();
        sim.update(&v, Stamp::seconds(201), 0.);
        v.completion_episodes = std::sync::Arc::new(
            sim.activities
                .keys()
                .enumerate()
                .map(|(i, k)| (k.clone(), i as u64 + 1))
                .collect(),
        );
        v.diagnostics = Some(log.clone());
        sim = Simulation::default();
        sim.update(&v, Stamp::seconds(201), 0.);
        sim.update(&v, Stamp::seconds(201), 2.);
        let ctx = egui::Context::default();
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(1100., 600.));
        let mut state = Panels::default();
        let frame = |sim: &Simulation, v: &View, state: &mut Panels| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    ..Default::default()
                },
                |ui| {
                    draw(
                        ui,
                        &Context {
                            sim,
                            view: v,
                            rect,
                            selected: None,
                            passive: false,
                        },
                        state,
                    );
                },
            );
            let count = output.shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "AGENT COMPLETED")).count();
            output.textures_delta.clear();
            count
        };
        let read = || {
            log.flush();
            std::fs::read_to_string(log.path())
                .unwrap()
                .lines()
                .map(|s| serde_json::from_str::<Value>(s).unwrap())
                .collect::<Vec<_>>()
        };
        let painted = frame(&sim, &v, &mut state);
        let first = read();
        assert!(painted > 0);
        assert_eq!(
            first
                .iter()
                .filter(|r| r["event"] == "completion_flyout" && r["data"]["reason"] == "drawn")
                .count(),
            painted
        );
        assert!(
            first
                .iter()
                .any(|r| r["event"] == "completion_flyout" && r["data"]["reason"] == "off_page")
        );
        assert!(first.iter().any(|r| r["event"] == "activity_changed"));
        assert!(
            first
                .iter()
                .filter(|r| r["event"] == "activity_changed" || r["event"] == "completion_flyout")
                .all(|r| r["data"]["renderer"].as_u64() == sim.diagnostic_renderer)
        );
        for _ in 0..20 {
            frame(&sim, &v, &mut state);
        }
        assert_eq!(
            read().len(),
            first.len(),
            "stable rendering must not create per-frame records"
        );
        let key = state
            .diagnostic_flyouts
            .iter()
            .find(|(_, t)| t.reason == "drawn")
            .unwrap()
            .0
            .clone();
        let episode = v.completion_episodes[&key];
        state.dismissed.insert(
            key.clone(),
            Dismissal {
                episode: Some(episode),
                at: 0.,
            },
        );
        frame(&sim, &v, &mut state);
        assert_eq!(state.diagnostic_flyouts[&key].reason, "acknowledged");
        assert_eq!(sim.summary().states[2], 12);
        // Admission, identity mismatch and unusably small views have explicit reasons.
        let other = state
            .diagnostic_flyouts
            .iter()
            .find(|(_, t)| t.reason == "drawn")
            .unwrap()
            .0
            .clone();
        let context = Context {
            sim: &sim,
            view: &v,
            rect,
            selected: None,
            passive: false,
        };
        state.trace_flyouts(&context, &cards(&sim), 0, 0, None);
        assert_eq!(state.diagnostic_flyouts[&other].reason, "no_layout_space");
        sim.activities.get_mut(&other).unwrap().stop_episode = Some(999);
        frame(&sim, &v, &mut state);
        assert_eq!(
            state.diagnostic_flyouts[&other].reason,
            "activity_episode_mismatch"
        );
        sim.activities.remove(&other);
        frame(&sim, &v, &mut state);
        assert_eq!(state.diagnostic_flyouts[&other].reason, "activity_missing");
        state.trace_unavailable(&sim, &v, true, rect);
        assert!(
            state
                .diagnostic_flyouts
                .values()
                .all(|t| t.reason == "passive_unavailable")
        );
        assert!(read().iter().any(|r| {
            r["event"] == "completion_flyout"
                && r["data"]["episode"]
                    .as_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    == Some(episode)
                && r["data"]["reason"] == "acknowledged"
        }));
    }
    #[test]
    fn restored_acknowledgement_is_hidden_from_first_frame_in_both_launchers() {
        for passive in [false, true] {
            let mut v = view(vec![node("one", "done", 1)], 1);
            let mut sim = Simulation::default();
            sim.update(&v, Stamp::seconds(201), 0.);
            v.completion_episodes = std::sync::Arc::new(
                sim.activities
                    .keys()
                    .enumerate()
                    .map(|(i, key)| (key.clone(), i as u64 + 10))
                    .collect(),
            );
            let key = v.completion_episodes.keys().next().unwrap().clone();
            v.acknowledged_completions =
                std::sync::Arc::new(BTreeMap::from([(key.clone(), v.completion_episodes[&key])]));
            sim = Simulation::default();
            sim.update(&v, Stamp::seconds(201), 0.);
            sim.update(&v, Stamp::seconds(201), 2.);
            let ctx = egui::Context::default();
            let rect = Rect::from_min_size(Pos2::ZERO, vec2(1100., 800.));
            let mut state = Panels::default();
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    ..Default::default()
                },
                |ui| {
                    draw(
                        ui,
                        &Context {
                            sim: &sim,
                            view: &v,
                            rect,
                            selected: None,
                            passive,
                        },
                        &mut state,
                    );
                },
            );
            assert_eq!(output.shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "AGENT COMPLETED")).count(), 1);
            assert_eq!(sim.summary().states[2], 2);
            assert!(
                !state
                    .membership
                    .contains(&CardId::Activity(sim.activities[&key].event.serial))
            );
            output.textures_delta.clear();
            // A new instance acknowledged in another renderer replaces the old local ID.
            std::sync::Arc::make_mut(&mut v.completion_episodes).insert(key.clone(), 99);
            std::sync::Arc::make_mut(&mut v.acknowledged_completions).insert(key.clone(), 99);
            v.acknowledgement_revision += 1;
            state.refresh_dismissals(&v);
            assert_eq!(state.dismissed[&key].episode, Some(99));
        }
    }
    #[test]
    fn dismiss_click_acknowledges_one_completion_without_hiding_counts_or_focus() {
        let diagnostics_dir = tempfile::tempdir().unwrap();
        let diagnostics =
            herdr_mesh_visualizer::diagnostics::Diagnostics::at(diagnostics_dir.path(), 8790)
                .unwrap();
        let ctx = egui::Context::default();
        let mut v = view(vec![node("one", "done", 1)], 1);
        let mut sim = Simulation::default();
        let mut state = Panels {
            focus: Some(v.scene.as_ref().unwrap().nodes[0].key.clone()),
            ..Default::default()
        };
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(1100., 800.));
        sim.update(&v, Stamp::seconds(201), 0.);
        v.completion_episodes = std::sync::Arc::new(
            sim.activities
                .keys()
                .enumerate()
                .map(|(i, k)| (k.clone(), i as u64 + 1))
                .collect(),
        );
        v.diagnostics = Some(diagnostics.clone());
        sim = Simulation::default();
        sim.update(&v, Stamp::seconds(201), 0.);
        sim.update(&v, Stamp::seconds(201), 2.);
        let draw_frame = |sim: &Simulation, v: &View, state: &mut Panels, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    events,
                    ..Default::default()
                },
                |ui| {
                    draw(
                        ui,
                        &Context {
                            sim,
                            view: v,
                            rect,
                            selected: None,
                            passive: false,
                        },
                        state,
                    );
                },
            );
            let dismiss = output.shapes.iter().find_map(|s| match &s.shape {
                egui::Shape::LineSegment { points, .. }
                    if points[1] - points[0] == vec2(7., 7.) =>
                {
                    Some(points[0] + vec2(3.5, 3.5))
                }
                _ => None,
            });
            let count = output.shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "AGENT COMPLETED")).count();
            output.textures_delta.clear();
            (dismiss, count)
        };
        let (button, count) = draw_frame(&sim, &v, &mut state, vec![]);
        assert_eq!(count, 2);
        let p = button.unwrap();
        for pressed in [true, false] {
            draw_frame(
                &sim,
                &v,
                &mut state,
                vec![
                    egui::Event::PointerMoved(p),
                    egui::Event::PointerButton {
                        pos: p,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: Default::default(),
                    },
                ],
            );
        }
        assert_eq!(state.dismissed.len(), 1);
        let key = state.dismissed.keys().next().unwrap().clone();
        diagnostics.flush();
        let records: Vec<herdr_mesh_visualizer::diagnostics::Value> =
            std::fs::read_to_string(diagnostics.path())
                .unwrap()
                .lines()
                .map(|s| serde_json::from_str(s).unwrap())
                .collect();
        assert_eq!(
            records
                .iter()
                .filter(|r| r["event"] == "dismiss_input")
                .count(),
            1
        );
        assert!(records.iter().any(|r| {
            r["event"] == "dismiss_input"
                && r["data"]["key"] == json!(key)
                && r["data"]["episode"]
                    .as_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    == Some(v.completion_episodes[&key])
        }));
        assert_eq!(
            state.take_dismissals(),
            vec![(key.clone(), v.completion_episodes[&key])]
        );
        assert!(state.take_dismissals().is_empty());
        assert_eq!(
            state.dismissed[&key].episode,
            v.completion_episodes.get(&key).copied()
        );
        sim.update(&v, Stamp::seconds(201), 2.8);
        assert!(
            card_reveal(
                &cards(&sim)
                    .into_iter()
                    .find(|c| c.key == Some(&key))
                    .unwrap(),
                &Context {
                    sim: &sim,
                    view: &v,
                    rect,
                    selected: None,
                    passive: false
                },
                &state
            )
            .active()
        );
        sim.update(&v, Stamp::seconds(201), 4.);
        assert_eq!(draw_frame(&sim, &v, &mut state, vec![]).1, 1);
        assert_eq!(sim.activities.len(), 2);
        assert!(
            !state
                .membership
                .contains(&CardId::Activity(sim.activities[&key].event.serial)),
            "acknowledged card must leave pagination and hit masks"
        );
        assert_eq!(sim.summary().states[2], 2);
        assert!(state.focus.is_some());
        v.epoch = 2;
        v.revision = 2;
        sim.update(&v, Stamp::seconds(202), 5.);
        assert_eq!(draw_frame(&sim, &v, &mut state, vec![]).1, 1);
        assert!(state.dismissed.contains_key(&key));
        let mut changed = node("one", "done", 1);
        let inventory = if key[3].is_empty() {
            changed.herdr.as_mut().unwrap()
        } else {
            changed.sessions[0].herdr.as_mut().unwrap()
        };
        inventory.agents[0].agent_status = "working".into();
        let mut working = view(vec![changed], 3);
        working.epoch = 2;
        working.completion_episodes = std::sync::Arc::new(
            v.completion_episodes
                .iter()
                .filter(|(k, _)| *k != &key)
                .map(|(k, n)| (k.clone(), *n))
                .collect(),
        );
        sim.update(&working, Stamp::seconds(202), 6.);
        draw_frame(&sim, &working, &mut state, vec![]);
        assert!(state.dismissed.is_empty());
        v.revision = 4;
        std::sync::Arc::make_mut(&mut v.completion_episodes)
            .entry(key)
            .and_modify(|n| *n += 10);
        sim.update(&v, Stamp::seconds(202), 7.);
        sim.update(&v, Stamp::seconds(202), 9.);
        assert_eq!(draw_frame(&sim, &v, &mut state, vec![]).1, 2);
    }
    #[test]
    fn a_new_completion_cannot_inherit_dismissal_when_working_was_not_drawn() {
        for simulation_saw_working in [true, false] {
            let ctx = egui::Context::default();
            let mut initial = view(vec![node("one", "done", 1)], 1);
            let mut sim = Simulation::default();
            sim.update(&initial, Stamp::seconds(201), 0.);
            initial.completion_episodes = std::sync::Arc::new(
                sim.activities
                    .keys()
                    .enumerate()
                    .map(|(i, k)| (k.clone(), i as u64 + 1))
                    .collect(),
            );
            sim = Simulation::default();
            sim.update(&initial, Stamp::seconds(201), 0.);
            sim.update(&initial, Stamp::seconds(201), 2.);
            let key = sim.activities.keys().next().unwrap().clone();
            let previous_serial = sim.activities[&key].event.serial;
            let mut state = Panels::default();
            for (key, episode) in initial.completion_episodes.iter() {
                state.dismissed.insert(
                    key.clone(),
                    Dismissal {
                        at: 2.,
                        episode: Some(*episode),
                    },
                );
            }
            state.refresh_dismissals(&initial);
            if simulation_saw_working {
                let working = view(vec![node("one", "working", 1)], 2);
                sim.update(&working, Stamp::seconds(202), 4.);
                assert_eq!(sim.activities[&key].state, AgentState::Working);
            }
            // The receive-time tracker supplies new tokens even if Working was never drawn/read.
            let mut completed = view(vec![node("one", "done", 1)], 3);
            completed.completion_episodes = std::sync::Arc::new(
                initial
                    .completion_episodes
                    .iter()
                    .map(|(k, n)| (k.clone(), n + 10))
                    .collect(),
            );
            sim.update(&completed, Stamp::seconds(203), 6.);
            assert!(sim.activities[&key].event.serial > previous_serial);
            assert_eq!(sim.activities[&key].event.started, 6.);
            sim.update(&completed, Stamp::seconds(203), 8.);
            let rect = Rect::from_min_size(Pos2::ZERO, vec2(1100., 800.));
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    ..Default::default()
                },
                |ui| {
                    draw(
                        ui,
                        &Context {
                            sim: &sim,
                            view: &completed,
                            rect,
                            selected: None,
                            passive: false,
                        },
                        &mut state,
                    );
                },
            );
            let count = output.shapes.iter().filter(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text() == "AGENT COMPLETED")).count();
            output.textures_delta.clear();
            assert_eq!(
                count, 2,
                "both scoped instances need new cards (simulation saw Working: {simulation_saw_working})"
            );
            assert!(state.dismissed.is_empty());
            assert_eq!(sim.summary().states[2], 2);
        }
    }
    #[test]
    fn acknowledgements_survive_sampled_history_readmission_but_not_removal_or_source_reset() {
        let mut sim = Simulation::default();
        let mut v = view(vec![node("one", "done", 3999)], 1);
        sim.update(&v, Stamp::seconds(201), 0.);
        let key = sim
            .activities
            .keys()
            .find(|k| sim.id(k).is_none())
            .unwrap()
            .clone();
        let mut state = Panels::default();
        v.completion_episodes = std::sync::Arc::new(BTreeMap::from([(key.clone(), 7)]));
        state.dismissed.insert(
            key.clone(),
            Dismissal {
                at: 0.,
                episode: Some(7),
            },
        );
        state.refresh_dismissals(&v);
        let serial = sim.activities.remove(&key).unwrap().event.serial;
        v.revision = 2;
        sim.update(&v, Stamp::seconds(202), 10.);
        state.refresh_dismissals(&v);
        assert!(sim.activities[&key].event.serial > serial);
        assert!(state.dismissed.contains_key(&key));
        assert_eq!(sim.summary().agents, 7998);
        state.refresh_dismissals(&view(vec![], 3));
        assert!(state.dismissed.is_empty());
        state.dismissed.insert(
            key,
            Dismissal {
                at: 0.,
                episode: None,
            },
        );
        state.reset_source();
        assert!(state.dismissed.is_empty());
    }
    #[test]
    fn completed_startup_cards_render_after_one_minute_with_work_hidden_and_focus_available() {
        let ctx = egui::Context::default();
        let v = view(vec![node("one", "done", 1)], 1);
        let mut sim = Simulation::default();
        let mut state = Panels::default();
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(1100., 800.));
        sim.update(&v, Stamp::seconds(201), 0.);
        state.controls.toggle_workers(2.);
        sim.update(&v, Stamp::seconds(202), 60.);
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect),
                ..Default::default()
            },
            |ui| {
                draw(
                    ui,
                    &Context {
                        sim: &sim,
                        view: &v,
                        rect,
                        selected: None,
                        passive: false,
                    },
                    &mut state,
                );
            },
        );
        for expected in [
            "AGENT COMPLETED",
            "Actual node one",
            "Actual workspace",
            "Actual agent 0",
        ] {
            assert!(output.shapes.iter().any(|s| matches!(&s.shape, egui::Shape::Text(t) if t.galley.text().contains(expected))), "missing {expected}");
        }
        assert!(
            sim.activities
                .values()
                .all(|a| a.persistent && a.state == AgentState::Completed)
        );
        assert!(!state.controls.workers.shown);
        output.textures_delta.clear();
    }
    #[test]
    fn working_toggle_hides_all_work_without_changing_counts_and_stops_still_show() {
        let ctx = egui::Context::default();
        let mut v = view(vec![node("one", "working", 2)], 1);
        let mut sim = Simulation::default();
        let mut state = Panels::default();
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(1100., 800.));
        sim.update(&v, Stamp::seconds(201), 0.);
        let before = sim.summary();
        state.controls.toggle_workers(2.);
        sim.update(&v, Stamp::seconds(201), 4.);
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect),
                ..Default::default()
            },
            |ui| {
                let response = draw(
                    ui,
                    &Context {
                        sim: &sim,
                        view: &v,
                        rect,
                        selected: None,
                        passive: false,
                    },
                    &mut state,
                );
                assert_eq!(response.blocked.len(), 2); // Only key and counts, no ghost work masks.
            },
        );
        assert!(!output.shapes.iter().any(
            |s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.text().contains("AGENT WORKING"))
        ));
        assert_eq!(sim.summary().agents, before.agents);
        assert_eq!(sim.summary().states, before.states);
        assert_eq!(sim.activities.values().filter(|a| a.persistent).count(), 4);
        output.textures_delta.clear();
        v = view(vec![node("one", "done", 2)], 2);
        sim.update(&v, Stamp::seconds(201), 5.);
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect),
                ..Default::default()
            },
            |ui| {
                draw(
                    ui,
                    &Context {
                        sim: &sim,
                        view: &v,
                        rect,
                        selected: None,
                        passive: false,
                    },
                    &mut state,
                );
            },
        );
        output.textures_delta.clear();
        sim.update(&v, Stamp::seconds(201), 7.);
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect),
                ..Default::default()
            },
            |ui| {
                draw(
                    ui,
                    &Context {
                        sim: &sim,
                        view: &v,
                        rect,
                        selected: None,
                        passive: false,
                    },
                    &mut state,
                );
            },
        );
        assert!(output.shapes.iter().any(
            |s| matches!(&s.shape,egui::Shape::Text(t) if t.galley.text().contains("COMPLETE"))
        ));
        assert!(!state.controls.workers.shown);
        output.textures_delta.clear();
    }
    #[test]
    fn fresh_removal_returns_to_first_page_even_when_card_order_is_unchanged() {
        let ctx = egui::Context::default();
        let v = view(vec![node("one", "working", 20)], 1);
        let mut sim = Simulation::default();
        sim.update(&v, Stamp::seconds(201), 0.);
        sim.update(&v, Stamp::seconds(201), 90.);
        let mut state = Panels::default();
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(1100., 800.));
        let run = |sim: &Simulation, v: &View, state: &mut Panels| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    ..Default::default()
                },
                |ui| {
                    draw(
                        ui,
                        &Context {
                            sim,
                            view: v,
                            rect,
                            selected: None,
                            passive: false,
                        },
                        state,
                    );
                },
            );
            output.textures_delta.clear();
        };
        run(&sim, &v, &mut state);
        let original: Vec<_> = cards(&sim).iter().map(|c| c.key.unwrap().clone()).collect();
        state.page = 1;
        state.page_start = 5;
        state.previous.push(0);
        let mut changed = node("one", "working", 20);
        changed.herdr.as_mut().unwrap().agents.remove(0);
        let changed = view(vec![changed], 2);
        assert!(crate::orb_ui::selected_branch(&changed, &original[0]).is_none());
        sim.update(&changed, Stamp::seconds(202), 92.);
        assert_eq!(
            cards(&sim)
                .iter()
                .map(|c| c.key.unwrap().clone())
                .collect::<Vec<_>>(),
            original
        );
        run(&sim, &changed, &mut state);
        assert_eq!(
            state.page_start, 0,
            "A new removal notice must return to page one"
        );
        assert_eq!(state.page, 0);
        assert!(state.previous.is_empty());
    }
    #[test]
    fn narrow_pager_keeps_both_buttons_visible_and_pointer_reachable() {
        let ctx = egui::Context::default();
        let footer = Rect::from_min_size(pos2(30., 40.), vec2(40., 26.));
        let mut state = Panels {
            page: 1,
            page_start: 5,
            previous: vec![0],
            ..Default::default()
        };
        let run = |state: &mut Panels, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(320., 600.))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    let mut child = ui.new_child(
                        UiBuilder::new()
                            .id_salt("narrow-pager-review")
                            .max_rect(footer),
                    );
                    child.set_clip_rect(footer);
                    child.horizontal(|ui| page_controls(ui, state, 8000, 5, true, 90.));
                },
            );
            let mut arrows = Vec::new();
            for shape in &output.shapes {
                if let egui::Shape::Text(t) = &shape.shape
                    && ["‹", "›"].contains(&t.galley.text())
                {
                    let glyph = Rect::from_min_size(t.pos, t.galley.size());
                    assert!(
                        footer.contains_rect(glyph),
                        "pager arrow clipped: {glyph:?} {:?}",
                        shape.clip_rect
                    );
                    arrows.push((t.galley.text().to_string(), glyph.center()));
                }
            }
            output.textures_delta.clear();
            arrows
        };
        let arrows = run(&mut state, vec![]);
        assert_eq!(arrows.len(), 2);
        let next = arrows.iter().find(|(label, _)| label == "›").unwrap().1;
        let click = |point, pressed| {
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ]
        };
        run(&mut state, click(next, true));
        run(&mut state, click(next, false));
        assert_eq!(state.page_start, 10);
        let previous = run(&mut state, vec![])
            .iter()
            .find(|(label, _)| label == "‹")
            .unwrap()
            .1;
        run(&mut state, click(previous, true));
        run(&mut state, click(previous, false));
        assert_eq!(state.page_start, 5);
    }
    #[test]
    fn content_sizing_and_packing_are_bounded_and_pagination_still_works_without_key() {
        let ctx = egui::Context::default();
        let v = view(vec![node("one", "working", 20)], 1);
        let mut sim = Simulation::default();
        sim.update(&v, Stamp::seconds(201), 0.);
        sim.update(&v, Stamp::seconds(201), 90.);
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(1100., 800.));
        let mut state = Panels::default();
        let mut button = None;
        let mut output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(rect),
                ..Default::default()
            },
            |ui| {
                let context = Context {
                    sim: &sim,
                    view: &v,
                    rect,
                    selected: None,
                    passive: false,
                };
                let docks = dock_layout(&context, &mut state);
                let reserved: Vec<_> = docks.iter().map(|d| d.bounds).collect();
                let candidates = cards(&sim);
                let small = card_size(ui.painter(), &candidates[0], &sim, vec2(320., 240.), false);
                assert!(small.x < 300. && small.y < 180., "{small:?}");
                let slots = places(ui.painter(), &context, &candidates, 0, &reserved, true);
                for (i, slot) in slots.iter().enumerate() {
                    assert!(rect.contains_rect(slot.expand(8.)));
                    assert!(reserved.iter().all(|r| !r.intersects(slot.expand(8.))));
                    assert!(
                        slots[..i]
                            .iter()
                            .all(|r| !r.expand(8.).intersects(slot.expand(8.)))
                    );
                }
                draw(ui, &context, &mut state);
            },
        );
        for shape in &output.shapes {
            if let egui::Shape::Text(text) = &shape.shape
                && text.galley.text() == "›"
            {
                button = Some(text.pos + text.galley.size() * 0.5);
            }
        }
        output.textures_delta.clear();
        let button = button.expect("Activity pager remains accessible with hidden key");
        for pressed in [true, false] {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    events: vec![
                        egui::Event::PointerMoved(button),
                        egui::Event::PointerButton {
                            pos: button,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Default::default(),
                        },
                    ],
                    ..Default::default()
                },
                |ui| {
                    draw(
                        ui,
                        &Context {
                            sim: &sim,
                            view: &v,
                            rect,
                            selected: None,
                            passive: false,
                        },
                        &mut state,
                    );
                },
            );
            output.textures_delta.clear();
        }
        assert_eq!(state.page, 1);
        assert!(state.page_start > 0);
    }
    #[test]
    fn bulk_shortcuts_typed_selection_and_pointer_close_do_not_acknowledge_outcomes() {
        let ctx = egui::Context::default();
        let v = view(vec![node("one", "idle", 1)], 1);
        let mut sim = Simulation::default();
        sim.update(&v, Stamp::seconds(201), 0.);
        let selected = v.scene.as_ref().unwrap().nodes[0].children[0].children[0]
            .key
            .clone();
        let mut state = Panels::default();
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(1920., 1080.));
        let frame = |state: &mut Panels, sim: &Simulation, selection: Option<&Key>, events| {
            let mut result = Response::default();
            let mut out = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    events,
                    ..Default::default()
                },
                |ui| {
                    state.handle_input(ui, sim.clock, false);
                    result = draw(
                        ui,
                        &Context {
                            sim,
                            view: &v,
                            rect,
                            selected: selection,
                            passive: false,
                        },
                        state,
                    );
                },
            );
            let texts = out
                .shapes
                .iter()
                .filter_map(|s| {
                    if let egui::Shape::Text(t) = &s.shape {
                        Some(t.galley.text().to_owned())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>();
            out.textures_delta.clear();
            (result, texts)
        };
        frame(
            &mut state,
            &sim,
            None,
            vec![
                key(egui::Key::N, true, false),
                key(egui::Key::S, true, false),
                key(egui::Key::W, true, false),
            ],
        );
        assert_eq!(state.objects.cards.len(), 5);
        sim.update(&v, Stamp::seconds(201), 3.);
        let (_, texts) = frame(&mut state, &sim, Some(&selected), vec![]);
        assert_eq!(state.objects.cards.len(), 5);
        assert!(texts.iter().any(|t| t == "WORKSPACE"));
        assert!(!texts.iter().any(|t| t == "Observation"));
        let id = CardId::Object(selected.clone());
        let bounds = state.presentations[&id]
            .motion
            .sample(sim.clock)
            .0
            .translate(vec2(0., drift(sim.clock, 0., 8.)));
        // Selected card is first and owns the pager when the rest overflow.
        let pager = state.membership.len()
            > state
                .presentations
                .values()
                .filter(|p| p.visibility.shown)
                .count();
        let point = pos2(
            bounds.left() + 62.,
            bounds.bottom() - if pager { 46. } else { 20. },
        );
        frame(
            &mut state,
            &sim,
            Some(&selected),
            vec![
                egui::Event::PointerMoved(point),
                egui::Event::PointerButton {
                    pos: point,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: Default::default(),
                },
            ],
        );
        let (result, _) = frame(
            &mut state,
            &sim,
            Some(&selected),
            vec![egui::Event::PointerButton {
                pos: point,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        );
        assert!(
            result.clear_selection,
            "bracketed × must close the selected typed card"
        );
        assert!(state.take_dismissals().is_empty());
        assert!(state.dismissed.is_empty());
        sim.update(&v, Stamp::seconds(201), 3.2);
        frame(&mut state, &sim, None, vec![]);
        assert!(!state.objects.cards.contains_key(&selected));
        assert!(
            state.presentations[&id]
                .visibility
                .reveal(sim.clock)
                .active(),
            "close animates instead of popping"
        );
        sim.update(&v, Stamp::seconds(201), 5.);
        frame(&mut state, &sim, None, vec![]);
        assert!(!state.presentations.contains_key(&id));
        frame(&mut state, &sim, Some(&selected), vec![]);
        assert!(state.objects.cards.contains_key(&selected));
        assert_eq!(sim.summary().agents, 2);
    }
    #[test]
    fn callout_reflow_retargets_current_position_and_staggers_each_visible_page() {
        let ctx = egui::Context::default();
        let v = view(vec![node("one", "idle", 1)], 1);
        let mut sim = Simulation::default();
        sim.update(&v, Stamp::seconds(201), 10.);
        let mut state = Panels::default();
        state.objects.toggle(crate::orb_objects::Class::Session);
        state.objects.update(&sim, &v, None);
        let objects: Vec<_> = state.objects.cards.values().cloned().collect();
        let candidates: Vec<_> = objects
            .iter()
            .map(|o| Card {
                event: &o.event,
                key: Some(&o.key),
                persistent: true,
                working: false,
                attention: false,
                acknowledgeable: false,
                typed: Some(o.glyph),
                pinned: false,
            })
            .collect();
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(1100., 800.));
        let context = Context {
            sim: &sim,
            view: &v,
            rect,
            selected: None,
            passive: false,
        };
        let slots = vec![
            Rect::from_min_size(pos2(700., 20.), vec2(300., 200.)),
            Rect::from_min_size(pos2(700., 250.), vec2(300., 200.)),
        ];
        assert!(present_cards(&candidates, &slots, 0, &mut state, &context).is_empty());
        sim.clock = 10.1;
        let context = Context {
            sim: &sim,
            view: &v,
            rect,
            selected: None,
            passive: false,
        };
        let shown = present_cards(&candidates, &slots, 0, &mut state, &context);
        assert_eq!(shown.len(), 1);
        sim.clock = 12.;
        let context = Context {
            sim: &sim,
            view: &v,
            rect,
            selected: None,
            passive: false,
        };
        let shown = present_cards(&candidates, &slots, 0, &mut state, &context);
        assert_eq!(shown.len(), 2);
        let id = candidates[0].id();
        let before = state.presentations[&id].motion.sample(12.).0;
        let moved: Vec<_> = slots
            .iter()
            .map(|r| r.translate(vec2(-100., 80.)))
            .collect();
        present_cards(&candidates, &moved, 0, &mut state, &context);
        assert_eq!(state.presentations[&id].motion.sample(12.).0, before);
        let midway = state.presentations[&id].motion.sample(12.2).0;
        sim.clock = 12.2;
        let context = Context {
            sim: &sim,
            view: &v,
            rect,
            selected: None,
            passive: false,
        };
        present_cards(&candidates, &slots, 0, &mut state, &context);
        assert_eq!(state.presentations[&id].motion.sample(12.2).0, midway);
        assert_eq!(state.presentations[&id].motion.sample(13.).0, slots[0]);
        // Page membership drives retraction; retired pages have no input index.
        let exits = present_cards(&[], &[], 0, &mut state, &context);
        assert!(exits.iter().all(|(_, _, _, i)| i.is_none()));
        sim.clock = 14.;
        let context = Context {
            sim: &sim,
            view: &v,
            rect,
            selected: None,
            passive: false,
        };
        assert!(present_cards(&[], &[], 0, &mut state, &context).is_empty());
        assert!(state.presentations.is_empty());
        drop(ctx);
    }
    #[test]
    fn animation_budget_waits_without_dropping_or_acknowledging_pending_candidates() {
        let v = view(vec![node("one", "idle", 1)], 1);
        let mut sim = Simulation::default();
        sim.update(&v, Stamp::seconds(201), 10.);
        let mut state = Panels::default();
        let objects: Vec<_> = (0..192)
            .map(|i| crate::orb_objects::Object {
                key: vec!["coordinator".into(), format!("root-{i}")],
                glyph: mesh_orb::Glyph::Coordinator,
                event: Event {
                    serial: 0,
                    origin: crate::mesh_model::Id::Node(0),
                    started: 0.,
                    title: "COORDINATOR",
                    text: String::new(),
                    color: [1., 1., 1.],
                },
            })
            .collect();
        let cards: Vec<_> = objects
            .iter()
            .map(|o| Card {
                event: &o.event,
                key: Some(&o.key),
                persistent: true,
                working: false,
                attention: false,
                acknowledgeable: false,
                typed: Some(o.glyph),
                pinned: false,
            })
            .collect();
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(1100., 800.));
        let slots = vec![Rect::from_min_size(pos2(700., 20.), vec2(300., 200.)); 64];
        for (clock, start) in [(10., 0), (30., 0), (30., 64), (31., 64), (31., 128)] {
            sim.clock = clock;
            let context = Context {
                sim: &sim,
                view: &v,
                rect,
                selected: None,
                passive: false,
            };
            present_cards(&cards, &slots, start, &mut state, &context);
            assert!(state.presentations.len() <= MAX_PRESENTATIONS);
        }
        assert!(!state.presentations.contains_key(&cards[191].id()));
        sim.clock = 50.;
        let context = Context {
            sim: &sim,
            view: &v,
            rect,
            selected: None,
            passive: false,
        };
        present_cards(&cards, &slots, 128, &mut state, &context);
        assert!(state.presentations.contains_key(&cards[191].id()));
        assert!(state.take_dismissals().is_empty());
    }
}

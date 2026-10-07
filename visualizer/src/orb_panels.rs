//! Bounded floating HUD, observation and activity callouts for the Orb only.
use crate::{
    animation::Motion,
    mesh_legend,
    mesh_model::{AgentState, Event, Simulation, ease},
    mesh_orb, mesh_stats,
    orb_hud::{Controls, Panel, Reveal},
};
use egui::{Color32, FontId, Painter, Pos2, Rect, Stroke, Ui, UiBuilder, pos2, vec2};
use herdr_mesh_visualizer::{client::View, projection::Key};
use std::collections::{BTreeMap, BTreeSet};

const CYAN: Color32 = Color32::from_rgb(90, 216, 235);

#[derive(Default)]
pub struct Panels {
    pub page: usize,
    pub projects: bool,
    pub focus: Option<Key>,
    dismissed: BTreeMap<Key, f64>,
    dismissal_revision: Option<(u64, u64)>,
    controls: Controls,
    key_motion: Option<Motion>,
    count_motion: Option<Motion>,
    page_start: usize,
    previous: Vec<usize>,
    membership: Vec<u64>,
    page_since: f64,
}
impl Panels {
    pub fn reset_source(&mut self) {
        self.page = 0;
        self.page_start = 0;
        self.previous.clear();
        self.membership.clear();
        self.projects = false;
        self.focus = None;
        self.dismissed.clear();
        self.dismissal_revision = None;
    }
    fn refresh_dismissals(&mut self, view: &View) {
        let revision = (view.epoch, view.revision);
        if self.dismissal_revision == Some(revision) {
            return;
        }
        self.dismissal_revision = Some(revision);
        if self.dismissed.is_empty() {
            return;
        }
        let mut retained = BTreeSet::new();
        fn scan(
            branch: &herdr_mesh_visualizer::projection::Branch,
            dismissed: &BTreeMap<Key, f64>,
            retained: &mut BTreeSet<Key>,
        ) {
            if branch.kind == "agent"
                && branch.status == "done"
                && dismissed.contains_key(&branch.key)
            {
                retained.insert(branch.key.clone());
            }
            for child in &branch.children {
                scan(child, dismissed, retained);
            }
        }
        if let Some(scene) = &view.scene {
            for node in &scene.nodes {
                scan(node, &self.dismissed, &mut retained);
            }
        }
        self.dismissed.retain(|key, _| retained.contains(key));
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
                        egui::Key::N => self.controls.toggle(Panel::Counts, clock),
                        egui::Key::W => self.controls.toggle_workers(clock),
                        egui::Key::Escape => self.focus = None,
                        _ => {}
                    }
                }
            }
        });
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

fn bounded(bounds: Rect, viewport: Rect) -> Rect {
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

struct Card<'a> {
    event: &'a Event,
    key: Option<&'a Key>,
    persistent: bool,
    working: bool,
    attention: bool,
}
fn cards(sim: &Simulation) -> Vec<Card<'_>> {
    let mut cards: Vec<_> = sim
        .activities
        .iter()
        .map(|(key, a)| Card {
            event: &a.event,
            key: Some(key),
            persistent: a.persistent,
            working: a.persistent && a.state == AgentState::Working,
            attention: a.persistent && a.state == AgentState::Blocked,
        })
        .collect();
    cards.extend(sim.events.iter().rev().map(|event| Card {
        event,
        key: None,
        persistent: false,
        working: false,
        attention: false,
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
    let id = match card.key {
        Some(key) => sim.anchor(key)?,
        None => card.event.origin,
    };
    mesh_orb::project_sim(mesh_orb::visible_position(sim, id), sim, rect)
}

fn retained(card: &Card<'_>, sim: &Simulation) -> bool {
    card.persistent && card.key.is_some_and(|key| sim.activity_retained(key))
}

fn title(card: &Card<'_>, sim: &Simulation) -> String {
    if retained(card, sim) {
        format!("{} · LAST KNOWN", card.event.title)
    } else {
        card.event.title.into()
    }
}
fn card_reveal(card: &Card<'_>, context: &Context<'_>, state: &Panels) -> Reveal {
    let own = Reveal::event(context.sim.clock - card.event.started, card.persistent);
    if let Some(since) = card.key.and_then(|key| state.dismissed.get(key)) {
        return Reveal::new(
            own.amount()
                .min((1. - (context.sim.clock - since) / crate::orb_hud::RETRACT) as f32),
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
        + 24.;
    let width = natural.max(if pager { 210. } else { 120. }).min(limit.x);
    let heading = painter.layout(
        title(card, sim),
        FontId::monospace(11.),
        CYAN,
        (width - 24.).max(1.),
    );
    let body = painter.layout(
        card.event.text.clone(),
        FontId::monospace(10.),
        CYAN,
        (width - 24.).max(1.),
    );
    vec2(
        width,
        (heading.size().y + body.size().y + 34. + 24. + if pager { 28. } else { 0. }).min(limit.y),
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

fn dismiss_completed(ui: &mut Ui, key: &Key, state: &mut Panels, clock: f64, compact: bool) {
    let response = ui.small_button(if compact { "×" } else { "Dismiss" });
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Button,
            ui.is_enabled(),
            "Dismiss completed notification",
        )
    });
    if response
        .on_hover_text("Acknowledge this completion; a later completion will appear again")
        .clicked()
    {
        state.dismissed.insert(key.clone(), clock);
    }
}

pub fn draw(ui: &mut Ui, context: &Context<'_>, state: &mut Panels) -> Response {
    state.refresh_dismissals(context.view);
    let rect = context.rect;
    let painter = ui.painter().with_clip_rect(rect);
    let docks = dock_layout(context, state);
    let mut response = Response::default();
    let selected = context
        .selected
        .and_then(|key| crate::orb_ui::selected_branch(context.view, key));
    let mut reserved: Vec<_> = docks.iter().map(|d| d.bounds).collect();
    if !context.passive
        && let Some(bounds) =
            crate::orb_focus::panel(ui, context.view, rect, &mut state.focus, &reserved)
    {
        reserved.push(bounds);
        response.blocked.push(bounds);
    }
    let detail = (selected.is_some() || state.projects)
        .then(|| detail_rect(context, &reserved))
        .filter(|r| r.width() >= 64. && r.height() >= 64.);
    if let Some(card) = detail {
        reserved.push(card);
        response.blocked.push(card);
    }
    let candidates: Vec<_> = cards(context.sim)
        .into_iter()
        .filter(|c| !c.working || state.controls.workers.occupies(context.sim.clock))
        .filter(|c| {
            card_reveal(c, context, state).active()
                || c.key.is_none_or(|key| !state.dismissed.contains_key(key))
        })
        .collect();
    if !state
        .membership
        .iter()
        .copied()
        .eq(candidates.iter().map(|c| c.event.serial))
    {
        state.membership = candidates.iter().map(|c| c.event.serial).collect();
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
    // All current overlay geometry must be known before projecting names.
    let mut label_obstacles = reserved.clone();
    label_obstacles.extend(slots.iter().zip(candidates.iter().skip(page_start)).map(
        |(slot, card)| {
            slot.translate(vec2(
                0.,
                drift(context.sim.clock, (card.event.serial % 3) as f64, 8.),
            ))
            .expand(3.)
        },
    ));
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
    for (slot, card) in slots.iter().zip(candidates.iter().skip(page_start)) {
        let bounds = slot.translate(vec2(
            0.,
            drift(context.sim.clock, (card.event.serial % 3) as f64, 8.),
        ));
        if let Some(anchor) = card_anchor(card, context.sim, rect) {
            let [r, g, b] = card.event.color;
            card_reveal(card, context, state).leader(
                &painter,
                anchor,
                bounds,
                egui::Rgba::from_rgb(r, g, b).into(),
            );
        }
    }

    if let Some(card) = detail {
        let anchor = context
            .selected
            .and_then(|key| {
                if selected.is_some_and(|b| b.kind == "coordinator") {
                    Some(mesh_orb::coordinator(context.sim.motion_time()))
                } else {
                    context
                        .sim
                        .id(key)
                        .map(|id| mesh_orb::visible_position(context.sim, id))
                }
            })
            .or_else(|| {
                state
                    .projects
                    .then(|| mesh_orb::coordinator(context.sim.motion_time()))
            })
            .and_then(|p| mesh_orb::project_sim(p, context.sim, rect));
        if let Some(anchor) = anchor {
            leader(&painter, anchor, card, CYAN);
        }
    }
    for (index, (slot, card)) in slots
        .iter()
        .zip(candidates.iter().skip(page_start))
        .enumerate()
    {
        let reveal = card_reveal(card, context, state);
        if !reveal.active() {
            continue;
        }
        let bounds = slot.translate(vec2(
            0.,
            drift(context.sim.clock, (card.event.serial % 3) as f64, 8.),
        ));
        let aperture = reveal.aperture(bounds).intersect(rect);
        response.blocked.push(aperture);
        let [r, g, b] = card.event.color;
        let color: Color32 = egui::Rgba::from_rgb(r, g, b).into();
        let opacity = reveal.opacity()
            * if retained(card, context.sim) {
                0.65
            } else {
                1.
            };
        let mut card_painter = painter.with_clip_rect(aperture);
        frame(&card_painter, aperture, color, ease(opacity + 0.25));
        card_painter.multiply_opacity(opacity);
        let heading = card_painter.layout(
            title(card, context.sim),
            FontId::monospace(11.),
            color,
            (bounds.width() - 24.).max(1.),
        );
        card_painter.galley(bounds.min + vec2(12., 12.), heading.clone(), color);
        let has_pager = index == 0 && pager && !context.passive;
        let body = Rect::from_min_max(
            bounds.min + vec2(12., heading.size().y + 22.),
            bounds.max - vec2(12., if has_pager { 64. } else { 36. }),
        );
        let mut child = ui.new_child(
            UiBuilder::new()
                .id(ui.id().with(("orb-activity", card.event.serial)))
                .max_rect(body),
        );
        child.set_clip_rect(body.intersect(aperture));
        if !reveal.interactive() {
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
                    .id(ui.id().with(("orb-activity-focus", card.event.serial)))
                    .max_rect(footer),
            );
            child.set_clip_rect(footer.intersect(aperture));
            if !reveal.interactive() {
                child.disable();
            }
            child.set_opacity(opacity);
            child.horizontal(|ui| {
                if let Some(key) = card.key.or_else(|| context.sim.key(card.event.origin)) {
                    let completed = card.persistent
                        && context
                            .sim
                            .activities
                            .get(key)
                            .is_some_and(|a| a.state == AgentState::Completed);
                    let compact = ui.available_width() < 100.;
                    if completed && compact {
                        dismiss_completed(ui, key, state, context.sim.clock, true);
                    }
                    // Removed activity keys do not inherit a recycled geometry slot.
                    if context.sim.anchor(key).is_some() {
                        crate::orb_focus::checkbox(ui, context.view, key, &mut state.focus);
                    }
                    if completed && !compact {
                        dismiss_completed(ui, key, state, context.sim.clock, false);
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
            ui.strong(if selected.is_some() {
                "Observation"
            } else {
                "Projects"
            });
            if !context.passive && ui.button("Close").clicked() {
                response.clear_selection = true;
                state.projects = false;
            }
        });
        if !context.passive
            && let Some(branch) = selected
        {
            crate::orb_focus::checkbox(&mut child, context.view, &branch.key, &mut state.focus);
        }
        egui::ScrollArea::vertical()
            .max_height(child.available_height())
            .auto_shrink([false, false])
            .show(&mut child, |ui| {
                if let Some(branch) = selected {
                    if let Some(scene) = &context.view.scene {
                        fn path<'a>(
                            branches: &'a [herdr_mesh_visualizer::projection::Branch],
                            key: &Key,
                            names: &mut Vec<&'a herdr_mesh_visualizer::projection::Branch>,
                        ) -> bool {
                            for branch in branches {
                                names.push(branch);
                                if &branch.key == key || path(&branch.children, key, names) {
                                    return true;
                                }
                                names.pop();
                            }
                            false
                        }
                        let mut names = Vec::new();
                        if path(&scene.nodes, &branch.key, &mut names) {
                            for ancestor in names.iter().take(names.len().saturating_sub(1)) {
                                ui.small(format!("{}: {}", ancestor.kind, ancestor.label));
                            }
                        }
                    }
                    crate::orb_ui::observations(ui, branch, context.view.live);
                } else if let Some(scene) = &context.view.scene {
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
                    ui.small("KEY · K");
                    if !context.passive && ui.selectable_label(state.projects, "Projects").clicked()
                    {
                        state.projects = !state.projects;
                        response.clear_selection = true;
                    }
                });
            } else {
                child.small("FLEET · N");
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
    // Sampling is relevant even when K/N are hidden; never disguise partial detail.
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
        run(vec![key(egui::Key::W, true, false)], false, 2., &mut state);
        assert!(!state.controls.workers.shown);
        run(
            vec![
                key(egui::Key::W, true, true),
                key(egui::Key::W, false, false),
            ],
            false,
            3.,
            &mut state,
        );
        assert!(!state.controls.workers.shown);
        run(vec![key(egui::Key::W, true, false)], true, 4., &mut state);
        assert!(!state.controls.workers.shown);
        run(
            vec![
                key(egui::Key::K, true, false),
                key(egui::Key::N, true, false),
            ],
            false,
            5.,
            &mut state,
        );
        state.reset_source();
        run(
            vec![key(egui::Key::W, false, false)],
            false,
            200.,
            &mut state,
        );
        assert!(!state.controls.key.reveal(200.).active());
        assert!(!state.controls.counts.reveal(200.).active());
        assert!(!state.controls.workers.reveal(200.).active());
        run(
            vec![key(egui::Key::W, true, false)],
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
    fn dismiss_click_acknowledges_one_completion_without_hiding_counts_or_focus() {
        let ctx = egui::Context::default();
        let mut v = view(vec![node("one", "done", 1)], 1);
        let mut sim = Simulation::default();
        let mut state = Panels {
            focus: Some(v.scene.as_ref().unwrap().nodes[0].key.clone()),
            ..Default::default()
        };
        let rect = Rect::from_min_size(Pos2::ZERO, vec2(1100., 800.));
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
                egui::Shape::Text(t) if t.galley.text() == "Dismiss" => {
                    Some(t.pos + t.galley.size() * 0.5)
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
                .contains(&sim.activities[&key].event.serial),
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
        sim.update(&working, Stamp::seconds(202), 6.);
        draw_frame(&sim, &working, &mut state, vec![]);
        assert!(state.dismissed.is_empty());
        v.revision = 4;
        sim.update(&v, Stamp::seconds(202), 7.);
        sim.update(&v, Stamp::seconds(202), 9.);
        assert_eq!(draw_frame(&sim, &v, &mut state, vec![]).1, 2);
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
        state.dismissed.insert(key.clone(), 0.);
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
        state.dismissed.insert(key, 0.);
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
}

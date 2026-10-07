//! Bounded floating HUD, observation and activity callouts for the Orb only.
use crate::{
    mesh_legend,
    mesh_model::{CALLOUT_REVEAL, Event, Simulation, callout_opacity, ease},
    mesh_orb, mesh_stats,
};
use egui::{Color32, FontId, Painter, Pos2, Rect, Stroke, Ui, UiBuilder, pos2, vec2};
use herdr_mesh_visualizer::{client::View, projection::Key};

const CYAN: Color32 = Color32::from_rgb(90, 216, 235);

#[derive(Default)]
pub struct Panels {
    pub page: usize,
    pub projects: bool,
    terminal_serial: u64,
    page_since: f64,
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
struct Hud {
    bounds: Rect,
    legend: Option<mesh_legend::Layout>,
    stats: mesh_stats::Layout,
    header_height: f32,
}

fn drift(clock: f64, phase: f64, amplitude: f32) -> f32 {
    ((clock * std::f64::consts::TAU / 48. + phase).sin() as f32) * amplitude
}

fn hud_layout(painter: &Painter, context: &Context<'_>) -> Hud {
    let rect = context.rect;
    let compact = rect.width() < 420. || rect.height() < 320.;
    let width = if compact {
        rect.width()
    } else {
        let key_width = mesh_legend::ENTRIES[..5]
            .iter()
            .map(|entry| {
                painter
                    .layout_no_wrap(entry.label.into(), FontId::monospace(10.), entry.color())
                    .size()
                    .x
                    + 64.
            })
            .sum::<f32>()
            + 56.;
        let stats_width = mesh_stats::preferred_width(painter, context.sim.summary()) + 40.;
        key_width.max(stats_width).min(820.).min(rect.width())
    };
    let header_height = if compact {
        24.
    } else {
        48. + if context.sim.omitted > 0 { 20. } else { 0. }
    };
    let measuring = Rect::from_min_size(Pos2::ZERO, vec2(width, rect.height()));
    let mut stats = if compact {
        mesh_stats::compact_layout(painter, measuring, context.sim.summary())
    } else {
        mesh_stats::layout(painter, measuring, context.sim.summary())
    };
    let mut legend =
        (!compact).then(|| mesh_legend::layout(painter, measuring, stats.bounds.top() - 8.));
    let top = legend
        .as_ref()
        .map_or(stats.bounds.top(), |key| key.bounds.top());
    let height = (stats.bounds.bottom() - top + header_height + 14.).min(rect.height());
    let amplitude = 12_f32.min(((rect.height() - height) * 0.5).max(0.));
    let y = rect.bottom() - height - amplitude + drift(context.sim.clock, 0., amplitude);
    let bounds = Rect::from_min_size(pos2(rect.left(), y.max(rect.top())), vec2(width, height));
    let offset = vec2(bounds.left(), bounds.top() + header_height - top);
    stats.translate(offset);
    if let Some(key) = &mut legend {
        key.translate(offset);
    }
    Hud {
        bounds,
        legend,
        stats,
        header_height,
    }
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

fn detail_rect(context: &Context<'_>, hud: &Hud) -> Rect {
    let rect = context.rect;
    let margin = 12_f32.min(rect.width() * 0.05).min(rect.height() * 0.05);
    let width = (rect.width() * 0.36)
        .clamp(240., 340.)
        .min((rect.width() - 2. * margin).max(1.));
    let x = rect.right() - width - margin;
    let bottom = if x < hud.bounds.right() + 12. {
        hud.bounds.top() - 18.
    } else {
        rect.bottom() - margin
    };
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

fn slots(rect: Rect, reserved: &[Rect], width: f32, height: f32) -> Vec<Rect> {
    let mut result = Vec::new();
    if width < 64. || height < 64. || width + 36. > rect.width() {
        return result;
    }
    let columns = if rect.width() >= width * 2. + 48. {
        2
    } else {
        1
    };
    for column in 0..columns {
        let x = if column == 0 {
            rect.right() - width - 18.
        } else {
            rect.left() + 18.
        };
        let mut y = rect.top() + 20.;
        while y + height + 12. <= rect.bottom() && result.len() < 64 {
            let slot = Rect::from_min_size(pos2(x, y), vec2(width, height));
            if !reserved.iter().any(|r| r.expand(14.).intersects(slot)) {
                result.push(slot);
            }
            y += height + 28.;
        }
    }
    result
}

struct Card<'a> {
    event: &'a Event,
    key: Option<&'a Key>,
    persistent: bool,
}
fn cards(sim: &Simulation) -> Vec<Card<'_>> {
    let mut cards: Vec<_> = sim
        .activities
        .iter()
        .map(|(key, a)| Card {
            event: &a.event,
            key: Some(key),
            persistent: a.persistent,
        })
        .collect();
    cards.extend(sim.events.iter().rev().map(|event| Card {
        event,
        key: None,
        persistent: false,
    }));
    // Short-lived transitions get first-page priority; persistent work remains pageable.
    cards.sort_by(|a, b| {
        a.persistent.cmp(&b.persistent).then_with(|| {
            if a.persistent {
                a.key.cmp(&b.key)
            } else {
                b.event.serial.cmp(&a.event.serial)
            }
        })
    });
    cards
}
fn card_anchor(card: &Card<'_>, sim: &Simulation, rect: Rect) -> Option<Pos2> {
    let id = match card.key {
        Some(key) => sim.id(key)?,
        None => card.event.origin,
    };
    mesh_orb::project(mesh_orb::visible_position(sim, id), sim.time, rect)
}

pub fn draw(ui: &mut Ui, context: &Context<'_>, state: &mut Panels) -> Response {
    let rect = context.rect;
    let painter = ui.painter().with_clip_rect(rect);
    let hud = hud_layout(&painter, context);
    let mut response = Response {
        blocked: vec![hud.bounds],
        clear_selection: false,
    };
    let selected = context
        .selected
        .and_then(|key| crate::orb_ui::selected_branch(context.view, key));
    let detail = (selected.is_some() || state.projects)
        .then(|| detail_rect(context, &hud))
        .filter(|r| r.width() >= 64. && r.height() >= 64.);
    let mut reserved = vec![hud.bounds];
    if let Some(card) = detail {
        reserved.push(card);
        response.blocked.push(card);
    }
    let candidates = cards(context.sim);
    let width = 300_f32
        .min(rect.width() * 0.38)
        .max(120.)
        .min((rect.width() - 36.).max(1.));
    // Bound layout work to visible cards, regardless of fleet size. Long names scroll.
    let height = 180_f32.min((rect.height() - 40.).max(1.));
    let places = slots(rect, &reserved, width, height);
    let capacity = places.len();
    let pages = candidates.len().div_ceil(capacity.max(1)).max(1);
    let terminal_serial = candidates
        .iter()
        .filter(|c| !c.persistent)
        .map(|c| c.event.serial)
        .max()
        .unwrap_or(state.terminal_serial);
    if terminal_serial > state.terminal_serial {
        state.terminal_serial = terminal_serial;
        state.page = 0;
        state.page_since = context.sim.clock;
    }
    if context.passive && context.sim.clock - state.page_since >= 12. {
        state.page = (state.page + 1) % pages;
        state.page_since = context.sim.clock;
    }
    state.page = state.page.min(pages - 1);

    // Leaders first, then cards and the HUD, so no line runs over panel text.
    if let Some(anchor) = mesh_orb::project(
        mesh_orb::coordinator(context.sim.time),
        context.sim.time,
        rect,
    ) {
        leader(&painter, anchor, hud.bounds, CYAN.gamma_multiply(0.5));
        painter.circle_stroke(
            anchor,
            14.,
            Stroke::new(1., Color32::from_rgb(242, 177, 70)),
        );
    }
    for (slot, card) in places
        .iter()
        .zip(candidates.iter().skip(state.page * capacity).take(capacity))
    {
        let card_rect = slot.translate(vec2(
            0.,
            drift(context.sim.clock, (card.event.serial % 3) as f64, 8.),
        ));
        response.blocked.push(card_rect);
        let age = (context.sim.clock - card.event.started) as f32;
        let opacity = if card.persistent {
            ease(age / CALLOUT_REVEAL)
        } else {
            callout_opacity(age)
        };
        let [r, g, b] = card.event.color;
        let color: Color32 = egui::Rgba::from_rgb(r, g, b).into();
        let retained = card.persistent
            && card
                .key
                .and_then(|key| context.sim.id(key))
                .is_some_and(|id| context.sim.opacity(id) < 1.);
        let color = color.gamma_multiply(opacity * if retained { 0.65 } else { 1. });
        if let Some(anchor) = card_anchor(card, context.sim, rect) {
            leader(&painter, anchor, card_rect, color);
        }
    }

    if let Some(card) = detail {
        let anchor = context
            .selected
            .and_then(|key| {
                if selected.is_some_and(|b| b.kind == "coordinator") {
                    Some(mesh_orb::coordinator(context.sim.time))
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
                    .then(|| mesh_orb::coordinator(context.sim.time))
            })
            .and_then(|p| mesh_orb::project(p, context.sim.time, rect));
        if let Some(anchor) = anchor {
            leader(&painter, anchor, card, CYAN);
        }
    }
    for (slot, card) in places
        .iter()
        .zip(candidates.iter().skip(state.page * capacity).take(capacity))
    {
        let card_rect = slot.translate(vec2(
            0.,
            drift(context.sim.clock, (card.event.serial % 3) as f64, 8.),
        ));
        let age = (context.sim.clock - card.event.started) as f32;
        let opacity = if card.persistent {
            ease(age / CALLOUT_REVEAL)
        } else {
            callout_opacity(age)
        };
        let [r, g, b] = card.event.color;
        let color: Color32 = egui::Rgba::from_rgb(r, g, b).into();
        let retained = card.persistent
            && card
                .key
                .and_then(|key| context.sim.id(key))
                .is_some_and(|id| context.sim.opacity(id) < 1.);
        let color = color.gamma_multiply(opacity * if retained { 0.65 } else { 1. });
        frame(&painter, card_rect, color, opacity);
        let title = if retained {
            format!("{} · LAST KNOWN", card.event.title)
        } else {
            card.event.title.into()
        };
        let heading = painter.layout(title, FontId::monospace(11.), color, (width - 24.).max(1.));
        painter.galley(card_rect.min + vec2(12., 12.), heading.clone(), color);
        let body = Rect::from_min_max(
            card_rect.min + vec2(12., heading.size().y + 22.),
            card_rect.max - vec2(12., 12.),
        );
        let mut child = ui.new_child(
            UiBuilder::new()
                .id_salt(("orb-activity", card.event.serial))
                .max_rect(body),
        );
        child.set_clip_rect(body.intersect(rect));
        egui::ScrollArea::vertical()
            .max_height(body.height())
            .auto_shrink([false, false])
            .show(&mut child, |ui| {
                ui.label(
                    egui::RichText::new(&card.event.text)
                        .monospace()
                        .size(10.)
                        .color(color),
                );
            });
    }

    if let Some(card) = detail {
        frame(&painter, card, CYAN, 1.);
        let body = card.shrink(12.).intersect(rect);
        let mut child = ui.new_child(
            UiBuilder::new()
                .id_salt("orb-observation-callout")
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

    frame(&painter, hud.bounds, CYAN, 1.);
    let header = Rect::from_min_size(
        hud.bounds.min + vec2(12., 6.),
        vec2(
            (hud.bounds.width() - 24.).max(1.),
            (hud.header_height - 8.).max(1.),
        ),
    );
    let mut child = ui.new_child(
        UiBuilder::new()
            .id_salt("orb-key-controls")
            .max_rect(header),
    );
    child.set_clip_rect(header.intersect(rect));
    child.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
    child.label(
        egui::RichText::new(if context.view.live {
            "LIVE · all known observations"
        } else {
            "LAST KNOWN · current fleet unavailable"
        })
        .monospace()
        .size(9.)
        .color(CYAN),
    );
    if hud.legend.is_some() {
        child.horizontal(|ui| {
            if !context.passive && ui.selectable_label(state.projects, "Projects").clicked() {
                state.projects = !state.projects;
                response.clear_selection = true;
            }
            if !candidates.is_empty() {
                ui.small(if capacity == 0 {
                    format!("{} callouts · enlarge view", candidates.len())
                } else {
                    format!(
                        "{} callouts · page {}/{}",
                        candidates.len(),
                        state.page + 1,
                        pages
                    )
                });
                if !context.passive && pages > 1 && capacity > 0 {
                    if ui.button("‹").clicked() {
                        state.page = (state.page + pages - 1) % pages;
                    }
                    if ui.button("›").clicked() {
                        state.page = (state.page + 1) % pages;
                    }
                }
            }
        });
        if context.sim.omitted > 0 {
            child.small(format!(
                "Detail sampled: {} omitted; totals complete",
                context.sim.omitted
            ));
        }
    }
    if let Some(key) = &hud.legend {
        mesh_legend::draw(&painter, key, context.sim.time);
    }
    mesh_stats::draw(&painter, &hud.stats);
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn drifting_hud_and_counts_stay_bounded_in_preview_and_regular_views() {
        use crate::mesh_model::tests::{node, view};
        let ctx = egui::Context::default();
        let view = view(vec![node("one", "working", 1)], 1);
        let mut sim = Simulation::default();
        sim.update(
            &view,
            herdr_mesh_visualizer::heartbeat::Stamp::seconds(201),
            2.,
        );
        for size in [
            vec2(180., 90.),
            vec2(320., 600.),
            vec2(500., 320.),
            vec2(1920., 1080.),
        ] {
            for clock in [0., 12., 24., 36., 48.] {
                sim.clock = clock;
                let rect = Rect::from_min_size(Pos2::ZERO, size);
                let mut output = ctx.run_ui(
                    egui::RawInput {
                        screen_rect: Some(rect),
                        ..Default::default()
                    },
                    |ui| {
                        let context = Context {
                            sim: &sim,
                            view: &view,
                            rect,
                            selected: None,
                            passive: false,
                        };
                        let hud = hud_layout(ui.painter(), &context);
                        assert!(rect.contains_rect(hud.bounds), "{size:?} {clock}");
                        assert!(hud.bounds.contains_rect(hud.stats.bounds));
                        if let Some(key) = &hud.legend {
                            assert!(hud.bounds.contains_rect(key.bounds));
                        }
                    },
                );
                output.textures_delta.clear();
            }
        }
    }
    #[test]
    fn card_slots_never_overlap_panels_or_each_other() {
        for size in [vec2(320., 240.), vec2(800., 600.), vec2(1920., 1080.)] {
            let rect = Rect::from_min_size(Pos2::ZERO, size);
            let reserved = [Rect::from_min_size(
                pos2(0., size.y - 160.),
                vec2(size.x.min(660.), 160.),
            )];
            let places = slots(rect, &reserved, 200., 140.);
            for (i, slot) in places.iter().enumerate() {
                assert!(rect.contains_rect(slot.expand(8.)));
                assert!(!reserved.iter().any(|r| r.intersects(slot.expand(8.))));
                assert!(
                    places[..i]
                        .iter()
                        .all(|r| !r.expand(8.).intersects(slot.expand(8.)))
                );
            }
        }
    }
}

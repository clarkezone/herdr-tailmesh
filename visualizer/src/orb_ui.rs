use crate::{mesh_model::Simulation, mesh_orb, orb_panels};
use egui::{Color32, Rect, Sense};
use herdr_mesh_visualizer::{
    client::View,
    projection::{Branch, Key, wall_now},
};

#[derive(Default)]
pub struct OrbUi {
    pub sim: Simulation,
    selected: Option<Key>,
    panels: orb_panels::Panels,
}
fn find<'a>(branches: &'a [Branch], key: &Key) -> Option<&'a Branch> {
    for b in branches {
        if &b.key == key {
            return Some(b);
        }
        if let Some(found) = find(&b.children, key) {
            return Some(found);
        }
    }
    None
}
pub(crate) fn selected_branch<'a>(view: &'a View, key: &Key) -> Option<&'a Branch> {
    let scene = view.scene.as_ref()?;
    scene
        .coordinator
        .as_ref()
        .filter(|b| &b.key == key)
        .or_else(|| find(&scene.nodes, key))
}
fn unavailable(ui: &mut egui::Ui) {
    let rect = ui.max_rect().intersect(ui.clip_rect());
    let color = Color32::from_gray(210);
    let mut galley = ui.painter().layout_no_wrap(
        "Daemon not available".into(),
        egui::FontId::proportional(24.),
        color,
    );
    let fit = ((rect.width() - 16.).max(1.) / galley.size().x)
        .min((rect.height() - 16.).max(1.) / galley.size().y)
        .min(1.);
    if fit < 1. {
        galley = ui.painter().layout_no_wrap(
            "Daemon not available".into(),
            egui::FontId::proportional(24. * fit),
            color,
        );
    }
    ui.painter()
        .galley(rect.center() - galley.size() / 2., galley, color);
}
impl OrbUi {
    pub fn draw(
        &mut self,
        root: &mut egui::Ui,
        view: &View,
        _port: u16,
        passive: bool,
        clock: f64,
    ) -> Option<Rect> {
        let generation = self.sim.source_generation;
        self.sim.update(view, wall_now(), clock);
        if self.sim.source_generation != generation {
            self.selected = None;
            self.panels = Default::default();
        }
        if self
            .selected
            .as_ref()
            .is_some_and(|key| selected_branch(view, key).is_none())
        {
            self.selected = None;
        }
        if passive && (!view.live || view.scene.is_none()) {
            self.selected = None;
            self.panels.projects = false;
            unavailable(root);
            return None;
        }
        if view.scene.is_none() {
            root.painter().text(
                root.max_rect().center(),
                egui::Align2::CENTER_CENTER,
                "Waiting for mesh observations…",
                egui::FontId::proportional(18.),
                Color32::from_gray(210),
            );
            return None;
        }
        let available = root.max_rect().intersect(root.clip_rect());
        let margin = 16_f32
            .min(available.width() * 0.05)
            .min(available.height() * 0.05);
        let rect = available.shrink(margin);
        if rect.width() < 1. || rect.height() < 1. {
            return None;
        }
        // The full symmetric viewport stays fixed when panels appear/disappear.
        let response = root.allocate_rect(
            rect,
            if passive {
                Sense::hover()
            } else {
                Sense::click()
            },
        );
        let panels = orb_panels::draw(
            root,
            &orb_panels::Context {
                sim: &self.sim,
                view,
                rect,
                selected: if passive {
                    None
                } else {
                    self.selected.as_ref()
                },
                passive,
            },
            &mut self.panels,
        );
        if panels.clear_selection {
            self.selected = None;
        }
        if !passive
            && response.clicked()
            && let Some(pointer) = response.interact_pointer_pos()
            && !panels.blocked.iter().any(|bounds| bounds.contains(pointer))
        {
            let picked = self.pick(view, pointer, rect);
            self.selected = if picked == self.selected {
                None
            } else {
                picked
            };
            self.panels.projects = false;
        }
        if !passive && let Some(key) = &self.selected {
            let position = if selected_branch(view, key).is_some_and(|b| b.kind == "coordinator") {
                Some(mesh_orb::coordinator(self.sim.time))
            } else {
                self.sim
                    .id(key)
                    .map(|id| mesh_orb::visible_position(&self.sim, id))
            };
            if let Some(point) = position.and_then(|p| mesh_orb::project(p, self.sim.time, rect)) {
                root.painter().with_clip_rect(rect).circle_stroke(
                    point,
                    14.,
                    egui::Stroke::new(1.5, Color32::from_rgb(220, 240, 255)),
                );
            }
        }
        root.ctx()
            .request_repaint_after(std::time::Duration::from_millis(16));
        Some(rect)
    }
    fn pick(&self, view: &View, pointer: egui::Pos2, rect: Rect) -> Option<Key> {
        let mut best = None;
        let mut distance = 14.0_f32;
        for (&id, life) in &self.sim.entities {
            if !life.entering() || life.alpha(self.sim.clock) < 0.1 {
                continue;
            }
            let Some(point) = mesh_orb::project(
                mesh_orb::visible_position(&self.sim, id),
                self.sim.time,
                rect,
            )
            .filter(|p| rect.contains(*p)) else {
                continue;
            };
            let d = point.distance(pointer);
            if d < distance {
                best = self.sim.key(id).cloned();
                distance = d;
            }
        }
        if let Some(root) = view
            .scene
            .as_ref()
            .and_then(|scene| scene.coordinator.as_ref())
            && let Some(point) =
                mesh_orb::project(mesh_orb::coordinator(self.sim.time), self.sim.time, rect)
            && point.distance(pointer) < distance
        {
            best = Some(root.key.clone());
        }
        best
    }
}
pub(crate) fn observations(ui: &mut egui::Ui, branch: &Branch, live: bool) {
    ui.label(format!("{} · {}", branch.kind, branch.label));
    ui.label(format!(
        "{} · {}",
        branch.status,
        if !live {
            "last known"
        } else if branch.kind == "coordinator" {
            "control connection live"
        } else {
            branch.freshness.label_at(wall_now())
        }
    ));
    if let Some(seen) = branch.last_seen {
        let now = wall_now();
        if seen <= now {
            ui.label(format!(
                "Last seen {}s ago",
                (now.nanos() - seen.nanos()) / 1_000_000_000
            ));
        } else {
            ui.label("Last seen unknown (clock ahead)");
        }
    }
    for line in &branch.details {
        ui.label(line);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mesh_model::tests::{node, view};
    fn text(shape: &egui::Shape, needle: &str) -> bool {
        match shape {
            egui::Shape::Text(t) => t.galley.text().contains(needle),
            egui::Shape::Vec(v) => v.iter().any(|s| text(s, needle)),
            _ => false,
        }
    }
    #[test]
    fn passive_disconnect_draws_only_unavailable_and_hides_retained_orb() {
        let context = egui::Context::default();
        let mut orb = OrbUi::default();
        let mut view = view(vec![node("one", "blocked", 1)], 1);
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800., 600.),
                )),
                ..Default::default()
            },
            |ui| {
                assert!(orb.draw(ui, &view, 8790, true, 0.).is_some());
            },
        );
        output.textures_delta.clear();
        view.live = false;
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(180., 90.))),
                ..Default::default()
            },
            |ui| {
                assert!(orb.draw(ui, &view, 8790, true, 1.).is_none());
            },
        );
        assert!(
            output
                .shapes
                .iter()
                .any(|s| text(&s.shape, "Daemon not available"))
        );
        for hidden in ["NODES", "Actual node", "Last-known", "HERDR MESH"] {
            assert!(!output.shapes.iter().any(|s| text(&s.shape, hidden)));
        }
        assert!(orb.sim.events.is_empty());
        assert!(orb.sim.pulses.is_empty());
        output.textures_delta.clear();
    }
    #[test]
    fn a_small_live_preview_keeps_the_orb_and_real_compact_totals() {
        let context = egui::Context::default();
        let mut orb = OrbUi::default();
        let view = view(vec![node("one", "working", 1)], 1);
        let viewport = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(180., 90.));
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(viewport),
                ..Default::default()
            },
            |ui| {
                let rect = orb.draw(ui, &view, 8790, true, 2.).unwrap();
                assert!(viewport.contains_rect(rect));
                assert!(rect.height() > 70.);
            },
        );
        assert!(output.shapes.iter().any(|s| text(&s.shape, "2 A")));
        assert!(
            !output
                .shapes
                .iter()
                .any(|s| text(&s.shape, "Daemon not available"))
        );
        output.textures_delta.clear();
    }
    #[test]
    fn floating_observation_persists_and_multiple_working_cards_are_rendered() {
        let context = egui::Context::default();
        let mut orb = OrbUi::default();
        let view = view(vec![node("one", "working", 2)], 1);
        orb.sim.update(
            &view,
            herdr_mesh_visualizer::heartbeat::Stamp::seconds(201),
            0.,
        );
        orb.selected = orb
            .sim
            .entities
            .keys()
            .find(|id| matches!(id, crate::mesh_model::Id::Agent(..)))
            .and_then(|id| orb.sim.key(*id))
            .cloned();
        let selected = orb.selected.clone();
        for clock in [2., 65.] {
            let viewport = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1920., 1080.));
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(viewport),
                    ..Default::default()
                },
                |ui| {
                    let rect = orb.draw(ui, &view, 8790, false, clock).unwrap();
                    assert_eq!(rect.center(), viewport.center());
                },
            );
            assert_eq!(orb.selected, selected);
            assert!(output.shapes.iter().any(|s| text(&s.shape, "Observation")));
            assert!(
                output
                    .shapes
                    .iter()
                    .filter(|s| text(&s.shape, "AGENT WORKING"))
                    .count()
                    >= 2
            );
            assert!(!output.shapes.iter().any(|s| text(&s.shape, "HERDR MESH")));
            output.textures_delta.clear();
        }
    }
    #[test]
    fn actual_pointer_clicks_open_observations_and_blank_clicks_deselect() {
        let context = egui::Context::default();
        let mut orb = OrbUi::default();
        let view = view(vec![node("one", "working", 1)], 1);
        orb.sim.update(
            &view,
            herdr_mesh_visualizer::heartbeat::Stamp::seconds(201),
            0.,
        );
        let frame = |orb: &mut OrbUi, events| {
            let mut rect = None;
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1100., 800.),
                    )),
                    events,
                    ..Default::default()
                },
                |ui| rect = orb.draw(ui, &view, 8790, false, 2.),
            );
            output.textures_delta.clear();
            rect.unwrap()
        };
        let rect = frame(&mut orb, vec![]);
        let id = *orb
            .sim
            .entities
            .keys()
            .find(|id| matches!(id, crate::mesh_model::Id::Node(_)))
            .unwrap();
        let point = mesh_orb::project(mesh_orb::visible_position(&orb.sim, id), orb.sim.time, rect)
            .unwrap();
        let click = |pos, pressed| {
            vec![
                egui::Event::PointerMoved(pos),
                egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ]
        };
        frame(&mut orb, click(point, true));
        frame(&mut orb, click(point, false));
        assert_eq!(orb.selected.as_ref(), orb.sim.key(id));
        let selected_rect = frame(&mut orb, vec![]);
        assert_eq!(selected_rect, rect); // Observation never moves/resizes the sphere.
        frame(&mut orb, click(point, true));
        frame(&mut orb, click(point, false));
        assert!(orb.selected.is_none()); // Same sphere object toggles off.
        frame(&mut orb, click(point, true));
        frame(&mut orb, click(point, false));
        assert!(orb.selected.is_some());
        let rect = frame(&mut orb, vec![]);
        let blank = rect.left_top() + egui::vec2(1., 1.);
        frame(&mut orb, click(blank, true));
        frame(&mut orb, click(blank, false));
        assert!(orb.selected.is_none());
    }
    #[test]
    fn real_footer_selection_and_retained_inventory_survive_resize() {
        let context = egui::Context::default();
        let mut orb = OrbUi::default();
        let mut view = view(vec![node("one", "idle", 1)], 1);
        orb.sim.update(
            &view,
            herdr_mesh_visualizer::heartbeat::Stamp::seconds(201),
            0.,
        );
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1100., 800.),
                )),
                ..Default::default()
            },
            |ui| {
                let rect = orb.draw(ui, &view, 8790, false, 2.).unwrap();
                let (&id, _) = orb
                    .sim
                    .entities
                    .iter()
                    .find(|(id, _)| matches!(id, crate::mesh_model::Id::Node(_)))
                    .unwrap();
                let point =
                    mesh_orb::project(mesh_orb::visible_position(&orb.sim, id), orb.sim.time, rect)
                        .unwrap();
                assert_eq!(orb.pick(&view, point, rect), orb.sim.key(id).cloned());
                orb.selected = orb.sim.key(id).cloned();
            },
        );
        for label in [
            "1 NODES",
            "2 SESSIONS",
            "2 WORKSPACES",
            "2 AGENTS",
            "0 WORKING",
            "0 BLOCKED",
            "0 COMPLETE",
        ] {
            assert!(output.shapes.iter().any(|s| text(&s.shape, label)));
        }
        output.textures_delta.clear();
        view.live = false;
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(320., 600.),
                )),
                ..Default::default()
            },
            |ui| {
                let rect = orb.draw(ui, &view, 8790, false, 3.).unwrap();
                assert!(rect.is_finite());
            },
        );
        assert!(orb.selected.is_some());
        assert!(output.shapes.iter().any(|s| text(&s.shape, "last known")));
        output.textures_delta.clear();
    }
}

use crate::{fleet_panel::FleetPanel, mesh_model::Simulation, mesh_orb};
use egui::{Color32, Rect, Sense};
use herdr_mesh_visualizer::{
    client::View,
    projection::{Branch, Key, wall_now},
    summary::summary,
};

#[derive(Default)]
pub struct OrbUi {
    pub sim: Simulation,
    selected: Option<Key>,
    fleet: FleetPanel,
    projects: bool,
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
fn selected_branch<'a>(view: &'a View, key: &Key) -> Option<&'a Branch> {
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
        port: u16,
        passive: bool,
        clock: f64,
    ) -> Option<Rect> {
        let generation = self.sim.source_generation;
        self.sim.update(view, wall_now(), clock);
        if self.sim.source_generation != generation {
            self.selected = None;
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
            unavailable(root);
            return None;
        }
        if passive && (root.available_height() < 320. || root.available_width() < 420.) {
            let rect = root.max_rect().intersect(root.clip_rect()).shrink(4.);
            if rect.width() < 1. || rect.height() < 1. {
                return None;
            }
            let painter = root.painter().with_clip_rect(rect);
            let color = Color32::from_rgb(90, 216, 235);
            painter.text(
                rect.left_top(),
                egui::Align2::LEFT_TOP,
                "HERDR MESH / LIVE",
                egui::FontId::monospace(8.),
                color,
            );
            let totals = self.sim.summary();
            let galley = painter.layout(
                format!(
                    "{} nodes · {} sessions · {} workspaces · {} agents",
                    totals.nodes, totals.sessions, totals.workspaces, totals.agents
                ),
                egui::FontId::monospace(8.),
                color,
                rect.width(),
            );
            painter.galley(
                rect.left_bottom() - egui::vec2(0., galley.size().y),
                galley,
                color,
            );
            return Some(rect);
        }
        // Selection follows scoped identity even if the display labels or slots change.
        let background = root.interact(
            root.max_rect(),
            root.id().with("orb-background"),
            Sense::click(),
        );
        let mut details_rect = None;
        let viewport=egui::Frame::NONE.inner_margin(16).show(root,|ui| {
            ui.set_min_size(ui.available_size());
            ui.set_clip_rect(ui.max_rect().intersect(ui.clip_rect()));
            ui.style_mut().wrap_mode=Some(egui::TextWrapMode::Wrap);
            if !passive {ui.heading("Herdr mesh · Orb");}
            ui.label(format!("127.0.0.1:{port} · {} · {}",if view.daemon.is_empty(){"daemon unknown"}else{&view.daemon},view.status));
            let Some(scene)=&view.scene else {ui.label("Waiting for mesh observations…");return None;};
            if let Some(root)=&scene.coordinator {ui.small(format!("Coordinator: {} · {}",root.label,root.status));}
            if ui.available_width()>700. && ui.available_height()>520. {
                egui::ScrollArea::vertical().id_salt("orb-fleet-overview").max_height(ui.available_height()*0.3).auto_shrink([false,true]).show(ui,|ui|self.fleet.draw(ui,view));
            } else {
                let counts=summary(scene,wall_now());
                if view.live {ui.small(format!("Fleet pulse: {} connected · {} fresh Herdr nodes · {} fresh agents",counts.fresh.connected,counts.fresh.fresh,counts.fresh.agents));}
                else {ui.small("Last-known inventory retained; current fleet counts unavailable.");}
            }
            ui.small(if view.live {"Footer: all known scoped observations · dim marks: stale/offline · neutral agents: idle/unknown"} else {"Footer: retained totals · all marks are last known"});
            if self.sim.omitted>0 {ui.colored_label(Color32::from_rgb(255,185,100),format!("{} detail marks omitted to fit GPU budget; totals include all observations. Use --tree for complete inventory.",self.sim.omitted));}
            if !passive {
                ui.horizontal(|ui| {ui.checkbox(&mut self.projects,"Project summary");if self.selected.is_some() && ui.button("Clear selection").clicked(){self.selected=None;}});
                if self.projects {
                    egui::ScrollArea::vertical().id_salt("orb-projects").max_height((ui.available_height()*0.2).min(120.)).show(ui,|ui| {
                        if scene.projects.is_empty() {ui.label("No reported project bindings");}
                        for project in &scene.projects {ui.label(format!("{} · {} workspace observations · {} nodes",project.id,project.workspaces,project.nodes));}
                        ui.small("Exact reported project IDs; no Git equivalence inferred.");
                    });
                }
                if let Some(branch)=self.selected.as_ref().and_then(|key|selected_branch(view,key)) {
                    if ui.available_width()>700. {
                        details_rect=Some(egui::Panel::right("orb-observations").exact_size(280.).frame(egui::Frame::NONE.inner_margin(12)).show(ui,|ui|observations(ui,branch,view.live)).response.rect);
                    } else {
                        details_rect=Some(egui::ScrollArea::vertical().id_salt("orb-observations-narrow").max_height((ui.available_height()*0.25).min(140.)).show(ui,|ui|observations(ui,branch,view.live)).inner_rect);
                    }
                }
            }
            let rect=ui.available_rect_before_wrap().intersect(ui.clip_rect());
            if rect.width()<1. || rect.height()<1. {return None;}
            let response=ui.allocate_rect(rect,if passive {Sense::hover()}else{Sense::click()});
            mesh_orb::overlay(ui,&self.sim,rect);
            if !passive && response.clicked() && let Some(pointer)=response.interact_pointer_pos() {
                self.selected=self.pick(view,pointer,rect);
            }
            if !passive && let Some(id)=self.selected.as_ref().and_then(|key|self.sim.id(key)) && let Some(p)=mesh_orb::project(mesh_orb::visible_position(&self.sim,id),self.sim.time,rect) {
                ui.painter().with_clip_rect(rect).circle_stroke(p,14.,egui::Stroke::new(1.5,Color32::from_rgb(220,240,255)));
            }
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(16));
            Some(rect)
        }).inner;
        if !passive
            && background.clicked()
            && !background
                .interact_pointer_pos()
                .is_some_and(|p| details_rect.is_some_and(|r| r.contains(p)))
        {
            self.selected = None;
        }
        viewport
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
fn observations(ui: &mut egui::Ui, branch: &Branch, live: bool) {
    ui.heading("Observation");
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
    egui::ScrollArea::vertical()
        .id_salt("orb-details-body")
        .show(ui, |ui| {
            for line in &branch.details {
                ui.label(line);
            }
        });
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
                let rect = orb.draw(ui, &view, 8790, true, 0.).unwrap();
                assert!(viewport.contains_rect(rect));
                assert!(rect.height() > 70.);
            },
        );
        assert!(output.shapes.iter().any(|s| text(&s.shape, "2 agents")));
        assert!(
            !output
                .shapes
                .iter()
                .any(|s| text(&s.shape, "Daemon not available"))
        );
        output.textures_delta.clear();
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

//! Scoped focus with one sampled camera shared by GPU geometry and UI projection.
use crate::{
    mesh_model::{Id, Simulation, ease},
    mesh_orb,
};
use egui::{Color32, FontId, Rect, Ui, UiBuilder, vec2};
use glam::{Quat, Vec3};
use herdr_mesh_visualizer::{
    client::View,
    projection::{Branch, Key},
};

const DURATION: f64 = 1.5;
const MAX_FOCUS_ZOOM: f32 = 7.2;
const COLOR: Color32 = Color32::from_rgb(90, 216, 235);
#[derive(Clone, Copy, Debug)]
pub struct Pose {
    pub rotation: Quat,
    pub center: Vec3,
    pub zoom: f32,
    pub orbit_time: f32,
}
impl Pose {
    pub fn ambient(time: f32) -> Self {
        Self {
            rotation: mesh_orb::scene_rotation(time),
            center: Vec3::ZERO,
            zoom: 1.,
            orbit_time: time,
        }
    }
    fn blend(self, other: Self, amount: f32) -> Self {
        Self {
            rotation: self.rotation.slerp(other.rotation, amount),
            center: self.center.lerp(other.center, amount),
            zoom: self.zoom + (other.zoom - self.zoom) * amount,
            orbit_time: other.orbit_time,
        }
    }
}
pub struct Controller {
    key: Option<Key>,
    from: Pose,
    target: Pose,
    since: f64,
    spin_offset: Quat,
    orbit_offset: f32,
}
impl Default for Controller {
    fn default() -> Self {
        Self {
            key: None,
            from: Pose::ambient(0.),
            target: Pose::ambient(0.),
            since: -DURATION,
            spin_offset: Quat::IDENTITY,
            orbit_offset: 0.,
        }
    }
}
impl Controller {
    pub fn update(&mut self, sim: &mut Simulation, requested: &mut Option<Key>, rect: Rect) {
        let now = sim.clock;
        let id = requested
            .as_ref()
            .and_then(|key| sim.id(key))
            .filter(|id| matches!(id, Id::Node(_)));
        if id.is_none() {
            *requested = None;
        }
        let changed = self.key != *requested;
        let current = sim.camera.unwrap_or_else(|| Pose::ambient(sim.time));
        if changed {
            self.from = current;
            self.since = now;
            if requested.is_none() {
                self.spin_offset = current.rotation * mesh_orb::scene_rotation(sim.time).inverse();
                self.orbit_offset = current.orbit_time - sim.time;
            }
            self.key = requested.clone();
        }
        let target = if let Some(id) = id {
            focused(
                sim,
                id.indices().0,
                if changed {
                    current.orbit_time
                } else {
                    self.target.orbit_time
                },
                rect,
            )
        } else {
            Pose {
                rotation: self.spin_offset * mesh_orb::scene_rotation(sim.time),
                orbit_time: sim.time + self.orbit_offset,
                ..Pose::ambient(sim.time)
            }
        };
        // Resize/topology changes retarget from what was actually displayed.
        if !changed
            && id.is_some()
            && ((target.zoom - self.target.zoom).abs() > 0.02
                || target.center.distance(self.target.center) > 0.01)
        {
            self.from = current;
            self.since = now;
        }
        self.target = target;
        sim.camera = Some(
            self.from
                .blend(target, ease(((now - self.since) / DURATION) as f32)),
        );
    }
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}
fn focused(sim: &Simulation, node: usize, orbit_time: f32, rect: Rect) -> Pose {
    let rotation = Quat::from_rotation_arc(crate::mesh_territory::anchor(node), Vec3::Z);
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for (&id, life) in &sim.entities {
        if id.indices().0 == node && life.entering() && !matches!(id, Id::Node(_)) {
            let p = rotation * mesh_orb::position(id, orbit_time) * 1.1;
            min = min.min(p);
            max = max.max(p);
        }
    }
    if !min.is_finite() {
        let p = rotation * crate::mesh_territory::anchor(node) * 1.94 * 1.1;
        min = p - Vec3::splat(0.4);
        max = p + Vec3::splat(0.4);
    }
    let center = (min + max) * 0.5;
    let extent = ((max - min) * 0.5 + Vec3::splat(0.20)).max(Vec3::splat(0.35));
    let aspect = (rect.width() / rect.height().max(1.)).max(0.01);
    // Leave edge columns for callouts and the full hierarchy list.
    let half = (45_f32.to_radians() * 0.5).tan();
    let distance =
        (extent.x / (half * aspect * 0.45)).max(extent.y / (half * 0.70)) + extent.z + 0.6;
    let viewport = crate::orb_viewport::Viewport {
        x: 0.,
        y: 0.,
        width: rect.width(),
        height: rect.height(),
        pixels_per_point: 1.,
    };
    Pose {
        rotation,
        center,
        zoom: (1.2 * mesh_orb::camera_distance(viewport) / distance.max(1.5))
            .clamp(1., MAX_FOCUS_ZOOM),
        orbit_time,
    }
}

pub fn node_for<'a>(view: &'a View, key: &Key) -> Option<&'a Branch> {
    view.scene
        .as_ref()?
        .nodes
        .iter()
        .find(|n| key.starts_with(&n.key))
}
pub fn checkbox(ui: &mut Ui, view: &View, key: &Key, focus: &mut Option<Key>) {
    let Some(node) = node_for(view, key) else {
        return;
    };
    let mut checked = focus.as_ref() == Some(&node.key);
    let (rect, mut response) = ui.allocate_exact_size(
        vec2(76_f32.min(ui.available_width().max(1.)), 20.),
        egui::Sense::click(),
    );
    if response.clicked() {
        checked = !checked;
        *focus = checked.then(|| node.key.clone());
        response.mark_changed();
    }
    response.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Checkbox,
            ui.is_enabled(),
            checked,
            "Focus",
        )
    });
    let amount = ui.ctx().animate_bool_with_time(response.id, checked, 0.4);
    let strength = if response.hovered() || response.has_focus() {
        1.
    } else {
        0.55 + 0.45 * amount
    };
    let color = COLOR.gamma_multiply(strength);
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        2.,
        Color32::from_rgba_unmultiplied(8, 35, 47, (35. + 50. * amount) as u8),
    );
    // Cut-corner rails and a lit targeting reticle instead of a stock tickbox.
    let r = rect.shrink(1.);
    for (corner, dx, dy) in [
        (r.left_top(), 1., 1.),
        (r.right_top(), -1., 1.),
        (r.left_bottom(), 1., -1.),
        (r.right_bottom(), -1., -1.),
    ] {
        painter.line_segment(
            [corner + vec2(dx * 7., 0.), corner],
            egui::Stroke::new(0.8, color),
        );
        painter.line_segment(
            [corner, corner + vec2(0., dy * 5.)],
            egui::Stroke::new(0.8, color),
        );
    }
    let center = egui::pos2(
        rect.left() + 13_f32.min(rect.width() * 0.5),
        rect.center().y,
    );
    let diamond = [
        center + vec2(0., -5.),
        center + vec2(5., 0.),
        center + vec2(0., 5.),
        center + vec2(-5., 0.),
    ];
    painter.add(egui::Shape::closed_line(
        diamond.to_vec(),
        egui::Stroke::new(0.8, color),
    ));
    for (a, b) in [
        (vec2(-8., 0.), vec2(-6., 0.)),
        (vec2(6., 0.), vec2(8., 0.)),
        (vec2(0., -8.), vec2(0., -6.)),
        (vec2(0., 6.), vec2(0., 8.)),
    ] {
        painter.line_segment([center + a, center + b], egui::Stroke::new(0.8, color));
    }
    if amount > 0.001 {
        painter.circle_filled(center, 4., COLOR.gamma_multiply(amount * 0.15));
        painter.circle_filled(center, 1.7, COLOR.gamma_multiply(amount));
        painter.line_segment(
            [
                egui::pos2(rect.left() + 27., rect.bottom() - 3.),
                egui::pos2(rect.left() + 27. + 38. * amount, rect.bottom() - 3.),
            ],
            egui::Stroke::new(0.8, COLOR.gamma_multiply(amount)),
        );
    }
    if rect.width() >= 62. {
        painter.text(
            egui::pos2(rect.left() + 27., rect.center().y),
            egui::Align2::LEFT_CENTER,
            "Focus",
            FontId::monospace(10.),
            color,
        );
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(
            "Bring this node's cluster forward and pause rotation; click again to return",
        );
}
/// Always-available exit and virtualized complete hierarchy, including sampled detail.
pub fn panel(
    ui: &mut Ui,
    sim: &Simulation,
    view: &View,
    rect: Rect,
    focus: &mut Option<Key>,
    reserved: &[Rect],
) -> Option<Rect> {
    let node = focus.as_ref().and_then(|key| node_for(view, key))?;
    let width = 240_f32.min((rect.width() - 24.).max(1.));
    let bottom = reserved
        .iter()
        .filter(|r| r.left() < rect.left() + 12. + width && r.right() > rect.left() + 12.)
        .map(|r| r.top() - 12.)
        .fold(rect.bottom() - 12., f32::min);
    let height = 320_f32
        .min(rect.height() * 0.45)
        .min((bottom - rect.top() - 12.).max(1.));
    let bounds = Rect::from_min_size(rect.left_top() + vec2(12., 12.), vec2(width, height));
    let painter = ui.painter().with_clip_rect(bounds);
    painter.rect_filled(bounds, 3., Color32::from_rgba_unmultiplied(4, 12, 25, 220));
    painter.rect_stroke(
        bounds,
        3.,
        egui::Stroke::new(1., COLOR),
        egui::StrokeKind::Inside,
    );
    if height < 40. {
        if height >= 16. {
            painter.text(
                bounds.left_top(),
                egui::Align2::LEFT_TOP,
                "Focus · Esc",
                FontId::monospace(10.),
                COLOR,
            );
            if ui
                .interact(bounds, ui.id().with("orb-focus-exit"), egui::Sense::click())
                .clicked()
            {
                *focus = None;
            }
        }
        return Some(bounds);
    }
    let mut child = ui.new_child(
        UiBuilder::new()
            .id_salt("orb-focus-hierarchy")
            .max_rect(bounds.shrink(10.)),
    );
    child.set_clip_rect(bounds.shrink(10.));
    child.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
    if child.small_button("Return to fleet · Esc").clicked() {
        *focus = None;
    }
    if height < 100. {
        child.small("Focus · enlarge view for hierarchy");
        return Some(bounds);
    }
    child.label(
        egui::RichText::new(format!("FOCUS · {}", node.label))
            .color(COLOR)
            .size(11.),
    );
    child.small(if !view.live {
        "Last known · disconnected"
    } else {
        "Rotation paused · full hierarchy"
    });
    let mut rows = Vec::new();
    fn flatten<'a>(branch: &'a Branch, depth: usize, rows: &mut Vec<(usize, &'a Branch)>) {
        rows.push((depth, branch));
        for b in &branch.children {
            flatten(b, depth + 1, rows);
        }
    }
    flatten(node, 0, &mut rows);
    // Fixed row heights support virtual scrolling without rendering all fleet names.
    egui::ScrollArea::both()
        .id_salt("orb-focus-rows")
        .auto_shrink([false, false])
        .max_height(child.available_height().max(1.))
        .show_rows(&mut child, 20., rows.len(), |ui, range| {
            for (depth, b) in &rows[range] {
                ui.horizontal(|ui| {
                    ui.add_space(*depth as f32 * 10.);
                    let label = format!("{}: {} · {}", b.kind, b.label, b.status);
                    ui.add(
                        egui::Label::new(egui::RichText::new(&label).monospace().size(10.))
                            .wrap_mode(egui::TextWrapMode::Extend),
                    )
                    .on_hover_text(format!(
                        "{label}\n{}",
                        if !view.live {
                            "last known".into()
                        } else {
                            b.freshness
                                .label_at(herdr_mesh_visualizer::projection::wall_now())
                                .to_string()
                        }
                    ));
                });
            }
        });
    // Labels beside projected glyphs; complete names remain accessible above.
    let Some(id) = sim.id(&node.key) else {
        return Some(bounds);
    };
    let mut used = vec![bounds];
    let mut labelled = 0;
    let mut measured = 0;
    for (&entity, life) in &sim.entities {
        if entity.indices().0 != id.indices().0
            || !life.entering()
            || life.alpha(sim.clock) < 0.1
            || labelled >= 128
            || measured >= 256
        {
            continue;
        }
        let Some(p) = mesh_orb::project_sim(mesh_orb::visible_position(sim, entity), sim, rect)
            .filter(|p| rect.contains(*p))
        else {
            continue;
        };
        measured += 1;
        let color = if matches!(entity, Id::Agent(..)) {
            egui::Rgba::from_rgb(
                sim.state(entity).color()[0],
                sim.state(entity).color()[1],
                sim.state(entity).color()[2],
            )
            .into()
        } else {
            COLOR
        };
        let galley =
            ui.painter()
                .layout_no_wrap(sim.name(entity).into(), FontId::monospace(10.), color);
        let label = Rect::from_min_size(p + vec2(10., -8.), galley.size());
        if rect.contains_rect(label) && !used.iter().any(|r| r.expand(3.).intersects(label)) {
            ui.painter().with_clip_rect(rect).rect_filled(
                label.expand(2.),
                2.,
                Color32::from_black_alpha(170),
            );
            ui.painter()
                .with_clip_rect(rect)
                .galley(label.min, galley, color);
            used.push(label);
            labelled += 1;
        }
    }
    Some(bounds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        mesh_model::tests::{node, view},
        orb_viewport::Viewport,
    };
    use herdr_mesh_visualizer::heartbeat::Stamp;
    fn advance(sim: &mut Simulation, view: &View, time: f64) {
        sim.update(view, Stamp::seconds(201), time);
    }
    fn same(a: Pose, b: Pose) {
        assert!(a.rotation.angle_between(b.rotation).abs() < 0.001);
        assert!(a.center.distance(b.center) < 0.00001);
        assert!((a.zoom - b.zoom).abs() < 0.00001);
        assert!((a.orbit_time - b.orbit_time).abs() < 0.00001);
    }
    #[test]
    fn focus_turns_cluster_front_freezes_motion_and_release_and_transfer_are_continuous() {
        let v = view(
            vec![node("one", "blocked", 2), node("two", "working", 1)],
            1,
        );
        let mut sim = Simulation::default();
        let mut control = Controller::default();
        let mut requested = None;
        let rect = Rect::from_min_size(egui::Pos2::ZERO, vec2(1100., 800.));
        advance(&mut sim, &v, 2.);
        control.update(&mut sim, &mut requested, rect);
        let before = sim.camera.unwrap();
        requested = Some(v.scene.as_ref().unwrap().nodes[0].key.clone());
        control.update(&mut sim, &mut requested, rect);
        same(before, sim.camera.unwrap());
        advance(&mut sim, &v, 4.);
        control.update(&mut sim, &mut requested, rect);
        let focused = sim.camera.unwrap();
        let n = sim.id(requested.as_ref().unwrap()).unwrap().indices().0;
        assert!((focused.rotation * crate::mesh_territory::anchor(n)).distance(Vec3::Z) < 0.0001);
        assert!(focused.zoom > 1.);
        let position = mesh_orb::visible_position(&sim, Id::Node(n));
        advance(&mut sim, &v, 100.);
        control.update(&mut sim, &mut requested, rect);
        same(focused, sim.camera.unwrap());
        assert_eq!(position, mesh_orb::visible_position(&sim, Id::Node(n)));
        assert_eq!(sim.time, 100.); // Effects and lifecycle clocks keep running.
        requested = Some(v.scene.as_ref().unwrap().nodes[1].key.clone());
        control.update(&mut sim, &mut requested, rect);
        same(focused, sim.camera.unwrap());
        advance(&mut sim, &v, 100.75);
        control.update(&mut sim, &mut requested, rect);
        let halfway = sim.camera.unwrap();
        requested = None;
        control.update(&mut sim, &mut requested, rect);
        same(halfway, sim.camera.unwrap());
        advance(&mut sim, &v, 103.);
        control.update(&mut sim, &mut requested, rect);
        assert_eq!(sim.camera.unwrap().zoom, 1.);
        assert_eq!(sim.camera.unwrap().center, Vec3::ZERO);
        assert!(sim.camera.unwrap().orbit_time > halfway.orbit_time);
    }
    #[test]
    fn focus_projection_matches_gpu_at_every_dpi_and_resize_stays_finite() {
        let v = view(vec![node("one", "blocked", 3)], 1);
        let mut sim = Simulation::default();
        advance(&mut sim, &v, 2.);
        let mut control = Controller::default();
        let mut requested = Some(v.scene.as_ref().unwrap().nodes[0].key.clone());
        for size in [
            vec2(1100., 800.),
            vec2(300., 900.),
            vec2(180., 88.),
            vec2(2400., 800.),
        ] {
            let rect = Rect::from_min_size(egui::pos2(16., 16.), size);
            control.update(&mut sim, &mut requested, rect);
            let next = sim.clock + 2.;
            advance(&mut sim, &v, next);
            control.update(&mut sim, &mut requested, rect);
            let pose = sim.camera.unwrap();
            assert!(pose.center.is_finite());
            assert!(pose.rotation.is_finite());
            assert!((1. ..=MAX_FOCUS_ZOOM).contains(&pose.zoom));
            let id = *sim
                .entities
                .keys()
                .find(|id| matches!(id, Id::Agent(..)))
                .unwrap();
            let p = mesh_orb::visible_position(&sim, id);
            let logical = mesh_orb::project_sim(p, &sim, rect).unwrap();
            for dpi in [1., 1.25, 1.5, 2., 3., 4.] {
                let viewport = Viewport {
                    x: rect.left() * dpi,
                    y: rect.top() * dpi,
                    width: rect.width() * dpi,
                    height: rect.height() * dpi,
                    pixels_per_point: dpi,
                };
                let (vp, model, _) = mesh_orb::camera_for(&sim, viewport);
                let clip = vp * model * p.extend(1.);
                let ndc = clip.truncate() / clip.w;
                let gpu = egui::pos2(
                    (viewport.x + (ndc.x + 1.) * viewport.width * 0.5) / dpi,
                    (viewport.y + (1. - ndc.y) * viewport.height * 0.5) / dpi,
                );
                assert!(gpu.distance(logical) < 0.001);
            }
        }
    }
    #[test]
    fn removal_clears_focus_before_a_recycled_node_slot_can_be_selected() {
        let mut sim = Simulation::default();
        let mut control = Controller::default();
        let first = view(vec![node("gone", "blocked", 1)], 1);
        let mut requested = Some(first.scene.as_ref().unwrap().nodes[0].key.clone());
        let rect = Rect::from_min_size(egui::Pos2::ZERO, vec2(1100., 800.));
        advance(&mut sim, &first, 2.);
        control.update(&mut sim, &mut requested, rect);
        advance(
            &mut sim,
            &view(vec![node("replacement", "working", 1)], 2),
            4.,
        );
        control.update(&mut sim, &mut requested, rect);
        assert!(requested.is_none());
        advance(
            &mut sim,
            &view(vec![node("replacement", "working", 1)], 2),
            6.,
        );
        control.update(&mut sim, &mut requested, rect);
        assert_eq!(sim.camera.unwrap().zoom, 1.);
    }
    #[test]
    fn focus_panel_stays_inside_tiny_and_docked_windows() {
        let ctx = egui::Context::default();
        let v = view(vec![node("one", "blocked", 1)], 1);
        let mut sim = Simulation::default();
        advance(&mut sim, &v, 2.);
        for size in [vec2(180., 88.), vec2(400., 300.), vec2(1100., 800.)] {
            let rect = Rect::from_min_size(egui::Pos2::ZERO, size);
            let dock = Rect::from_min_max(
                egui::pos2(0., size.y * 0.25),
                egui::pos2(190_f32.min(size.x), size.y),
            );
            let mut focus = Some(v.scene.as_ref().unwrap().nodes[0].key.clone());
            let mut output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(rect),
                    ..Default::default()
                },
                |ui| {
                    let bounds = panel(ui, &sim, &v, rect, &mut focus, &[dock]).unwrap();
                    assert!(bounds.is_finite());
                    assert!(rect.contains_rect(bounds));
                    assert!(!bounds.intersects(dock));
                },
            );
            output.textures_delta.clear();
        }
    }
    #[test]
    fn custom_reticle_keeps_tab_space_and_enter_checkbox_semantics() {
        let ctx = egui::Context::default();
        let v = view(vec![node("one", "blocked", 1)], 1);
        let mut focus = None;
        let key = &v.scene.as_ref().unwrap().nodes[0].key;
        let run = |focus: &mut Option<Key>, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| checkbox(ui, &v, key, focus),
            );
            output.textures_delta.clear();
        };
        let event = |key, pressed| {
            vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: false,
                modifiers: Default::default(),
            }]
        };
        run(&mut focus, vec![]);
        run(&mut focus, event(egui::Key::Tab, true));
        run(&mut focus, event(egui::Key::Tab, false));
        run(&mut focus, event(egui::Key::Space, true));
        assert_eq!(focus.as_ref(), Some(key));
        run(&mut focus, event(egui::Key::Space, false));
        run(&mut focus, event(egui::Key::Enter, true));
        assert!(focus.is_none());
    }
    #[test]
    fn real_checkbox_input_sets_scoped_focus_and_unchecks_it() {
        let ctx = egui::Context::default();
        let v = view(vec![node("one", "blocked", 1)], 1);
        let mut focus = None;
        let key = &v.scene.as_ref().unwrap().nodes[0].children[0].children[0].children[0].key;
        let run = |focus: &mut Option<Key>, events| {
            let mut output = ctx.run_ui(
                egui::RawInput {
                    events,
                    ..Default::default()
                },
                |ui| checkbox(ui, &v, key, focus),
            );
            let point = output
                .shapes
                .iter()
                .find_map(|s| match &s.shape {
                    egui::Shape::Text(t) if t.galley.text() == "Focus" => {
                        Some(t.pos + t.galley.size() * 0.5)
                    }
                    _ => None,
                })
                .unwrap();
            output.textures_delta.clear();
            point
        };
        let p = run(&mut focus, vec![]);
        let click = |pressed| {
            vec![
                egui::Event::PointerMoved(p),
                egui::Event::PointerButton {
                    pos: p,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Default::default(),
                },
            ]
        };
        run(&mut focus, click(true));
        run(&mut focus, click(false));
        assert_eq!(
            focus.as_ref(),
            Some(&v.scene.as_ref().unwrap().nodes[0].key)
        );
        run(&mut focus, click(true));
        run(&mut focus, click(false));
        assert!(focus.is_none());
    }
}

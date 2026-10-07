//! 3D constellation geometry and projected movie-style callouts.
use crate::mesh_model::{
    AgentState, CALLOUT_REVEAL, Id, PULSE_DURATION, Simulation, callout_opacity, ease, noise,
};
use crate::orb_viewport::Viewport;
use crate::orbital_sphere::{LineVertex, Particle};
use glam::{Mat4, Quat, Vec3};
use std::f32::consts::{PI, TAU};

pub const MAX_PARTICLES: usize = 60_000;
pub const MAX_LINES: usize = 48_000;
pub struct Geometry {
    pub particles: Vec<Particle>,
    pub lines: Vec<LineVertex>,
}

/// Semantic forms shared by the live scene and its visual key.
#[derive(Clone, Copy)]
pub enum Glyph {
    Coordinator,
    Node,
    Session,
    Workspace,
    Agent(AgentState),
}
impl Glyph {
    pub fn color(self) -> [f32; 3] {
        match self {
            Self::Coordinator => [1.0, 0.62, 0.12],
            Self::Node => [0.85, 0.92, 1.0],
            Self::Session => [0.7, 0.32, 1.0],
            Self::Workspace => [0.025, 0.22, 1.0],
            Self::Agent(state) => state.color(),
        }
    }
}

pub fn glyph_geometry(glyph: Glyph, time: f32) -> Geometry {
    let mut g = Geometry {
        particles: Vec::new(),
        lines: Vec::new(),
    };
    g.glyph(glyph, Vec3::Z, 1.0, time, 0.0);
    g
}

pub fn camera_distance(viewport: Viewport) -> f32 {
    let aspect = viewport.width / viewport.height.max(1.0);
    let half = (45.0_f32.to_radians() * 0.5).tan();
    3.55 / (half * aspect.clamp(0.01, 1.0)) + 0.5
}
fn scene_rotation(time: f32) -> Quat {
    Quat::from_euler(glam::EulerRot::XYZ, time * 0.018, time * 0.048, 0.0)
}
pub fn camera(time: f32, viewport: Viewport) -> (Mat4, Mat4, f32) {
    let aspect = viewport.width / viewport.height.max(1.0);
    let half = (45.0_f32.to_radians() * 0.5).tan();
    let z = camera_distance(viewport);
    let projection =
        glam::camera::rh::proj::directx::perspective(45_f32.to_radians(), aspect, 0.1, 1000.0);
    let view = glam::camera::rh::view::look_at_mat4(Vec3::new(0.0, 0.0, z), Vec3::ZERO, Vec3::Y);
    let model =
        Mat4::from_scale_rotation_translation(Vec3::splat(1.1), scene_rotation(time), Vec3::ZERO);
    (
        projection * view,
        model,
        // Size limits are logical points, just like the HUD and hit targets.
        // Clamp before converting to pixels or high-DPI markers hit the cap
        // early and shrink relative to the sphere and text.
        (viewport.height / viewport.pixels_per_point * 1.1 / (2.0 * z * half) / 105.0)
            .clamp(0.9, 1.5)
            * viewport.pixels_per_point,
    )
}
fn orbit_rotation(index: usize, time: f32) -> Quat {
    Quat::from_euler(
        glam::EulerRot::XYZ,
        noise(index as u32 + 83) * TAU,
        noise(index as u32 + 23) * TAU,
        time * 0.024 * if index.is_multiple_of(2) { 1.0 } else { -1.0 },
    )
}
fn orbit(index: usize, angle: f32, time: f32) -> Vec3 {
    let radius = 2.5 + noise(index as u32 + 47) * 0.35;
    orbit_rotation(index, time)
        * Vec3::new(
            angle.cos() * radius,
            angle.sin() * radius,
            (angle * 4.0).sin() * 0.1,
        )
}
pub fn coordinator(time: f32) -> Vec3 {
    orbit(5, 0.75 + time * 0.028, time)
}
fn basis(normal: Vec3) -> (Vec3, Vec3) {
    let axis = if normal.y.abs() > 0.95 {
        Vec3::X
    } else {
        Vec3::Y
    };
    let u = normal.cross(axis).normalize();
    (u, normal.cross(u).normalize())
}
fn circle(center: Vec3, radius: f32, angle: f32) -> Vec3 {
    let (u, v) = basis(center.normalize());
    center + radius * (u * angle.cos() + v * angle.sin())
}
// Dyadic angular insertion fills the whole ring for small child counts while
// preserving every earlier slot when children arrive or depart.
fn child_angle(slot: usize) -> f32 {
    ((slot as u32).reverse_bits() as f64 / 4_294_967_296.0) as f32 * TAU
}

pub fn position(id: Id, time: f32) -> Vec3 {
    let (n, s, w, a) = id.indices();
    if matches!(id, Id::Node(..)) {
        return orbit(
            n % 6,
            noise(n as u32 + 131) * TAU + time * (0.025 + noise(n as u32 + 91) * 0.018),
            time,
        );
    }
    let normal = crate::mesh_territory::anchor(n);
    let (u, v) = basis(normal);
    let session = (normal
        + 0.45 * (u * (child_angle(s) + 0.4).cos() + v * (child_angle(s) + 0.4).sin()))
    .normalize()
        * 1.92;
    if matches!(id, Id::Session(..)) {
        return session;
    }
    let workspace = circle(session, 0.42, child_angle(w) + 0.2);
    if matches!(id, Id::Workspace(..)) {
        return workspace;
    }
    circle(workspace, 0.15, child_angle(a) + 0.35)
}
pub fn visible_position(sim: &Simulation, id: Id) -> Vec3 {
    let pos = position(id, sim.time);
    let alpha = sim
        .entities
        .get(&id)
        .map_or(1.0, |life| life.alpha(sim.clock));
    pos * (1.0 + (1.0 - alpha) * 0.15)
}
fn arc(start: Vec3, end: Vec3, t: f32, bulge: f32) -> Vec3 {
    let center = start.lerp(end, t);
    let outward = (start + end).try_normalize().unwrap_or(Vec3::Y);
    center + outward * (PI * t).sin() * bulge
}
impl Geometry {
    fn glyph(&mut self, glyph: Glyph, p: Vec3, alpha: f32, time: f32, phase: f32) {
        match glyph {
            Glyph::Coordinator => {
                self.dot(p, 7.0, glyph.color(), alpha);
                for i in 0..32 {
                    self.dot(
                        circle(p, 0.085, i as f32 * TAU / 32.0 + time * 0.2),
                        1.15,
                        [1.0, 0.5, 0.08],
                        alpha * 0.7,
                    );
                }
            }
            Glyph::Node => self.dot(p, 5.5, glyph.color(), alpha),
            Glyph::Session => {
                self.dot(p, 4.8, glyph.color(), alpha);
                // Keep the enlarged ring tangent to the sphere's surface.
                // Its radial normal rotates with the session, preserving depth.
                for i in 0..24 {
                    let angle = i as f32 * TAU / 24.0;
                    self.dot(circle(p, 0.22, angle), 1.35, [0.5, 0.15, 0.9], alpha * 0.8);
                }
            }
            Glyph::Workspace => {
                let diamond =
                    std::array::from_fn::<_, 4, _>(|i| circle(p, 0.065, i as f32 * TAU / 4.0));
                for i in 0..4 {
                    self.line(diamond[i], diamond[(i + 1) % 4], glyph.color(), alpha * 0.8);
                }
                self.dot(p, 1.3, glyph.color(), alpha * 0.6);
            }
            Glyph::Agent(state) => {
                let breathing = match state {
                    AgentState::Working => 0.75 + 0.25 * (time * 2.8 + phase).sin(),
                    AgentState::Blocked => 0.85 + 0.15 * (time * 0.9).sin(),
                    AgentState::Completed => 0.6,
                    AgentState::Idle | AgentState::Unknown => 0.45,
                };
                // State breathing changes brightness, never the core's size.
                // Idle/unknown remains neutral without becoming a tiny speck.
                self.agent_dot(p, state.color().map(|channel| channel * breathing), alpha);
            }
        }
    }
    fn dot(&mut self, pos: Vec3, size: f32, color: [f32; 3], alpha: f32) {
        self.dot_with_halo(pos, size, color, alpha, 3.4, 0.13);
    }
    fn agent_dot(&mut self, pos: Vec3, color: [f32; 3], alpha: f32) {
        self.dot_with_halo(pos, 5.0, color, alpha, 1.6, 0.045);
    }
    fn dot_with_halo(
        &mut self,
        pos: Vec3,
        size: f32,
        color: [f32; 3],
        alpha: f32,
        halo_size: f32,
        halo_strength: f32,
    ) {
        if alpha <= 0.001 {
            return;
        }
        // Compact agent halos preserve the gap between adjacent status dots.
        self.particles.push(Particle {
            position_size: [pos.x, pos.y, pos.z, size * (0.4 + 0.6 * alpha)],
            color_softness: [color[0] * alpha, color[1] * alpha, color[2] * alpha, 0.92],
        });
        self.particles.push(Particle {
            position_size: [pos.x, pos.y, pos.z, size * halo_size],
            color_softness: [
                color[0] * alpha * halo_strength,
                color[1] * alpha * halo_strength,
                color[2] * alpha * halo_strength,
                0.03,
            ],
        });
    }
    fn line(&mut self, a: Vec3, b: Vec3, color: [f32; 3], alpha: f32) {
        let color = [color[0] * alpha, color[1] * alpha, color[2] * alpha, alpha];
        self.lines.push(LineVertex {
            position: a.to_array(),
            color,
        });
        self.lines.push(LineVertex {
            position: b.to_array(),
            color,
        });
    }
    fn curve(&mut self, a: Vec3, b: Vec3, color: [f32; 3], alpha: f32, bulge: f32) {
        for i in 0..24 {
            self.line(
                arc(a, b, i as f32 / 24.0, bulge),
                arc(a, b, (i + 1) as f32 / 24.0, bulge),
                color,
                alpha,
            );
        }
    }
}
pub fn geometry(sim: &Simulation) -> Geometry {
    let time = sim.time;
    let mut g = Geometry {
        particles: Vec::new(),
        lines: Vec::new(),
    };
    for ring in 0..6 {
        for i in 0..120 {
            g.line(
                orbit(ring, i as f32 * TAU / 120.0, time),
                orbit(ring, (i + 1) as f32 * TAU / 120.0, time),
                [0.21, 0.09, 0.5],
                0.6,
            );
        }
    }
    let root = coordinator(time);
    g.glyph(
        Glyph::Coordinator,
        root,
        if sim.live { 1.0 } else { 0.3 },
        time,
        0.0,
    );
    for (&id, life) in &sim.entities {
        let alpha = life.alpha(sim.clock) * sim.opacity(id);
        if alpha <= 0.001 {
            continue;
        }
        let p = visible_position(sim, id);
        match id {
            Id::Node(_) => {
                g.glyph(Glyph::Node, p, alpha, time, 0.0);
                g.curve(p, root, [0.3, 0.13, 0.7], alpha * 0.28, 0.5);
            }
            Id::Session(n, _) => {
                g.glyph(Glyph::Session, p, alpha, time, 0.0);
                g.curve(
                    p,
                    visible_position(sim, Id::Node(n)),
                    [0.25, 0.35, 0.7],
                    alpha * 0.3,
                    0.18,
                );
            }
            Id::Workspace(n, s, _) => {
                g.glyph(Glyph::Workspace, p, alpha, time, 0.0);
                g.line(
                    p,
                    visible_position(sim, Id::Session(n, s)),
                    [0.25, 0.2, 0.6],
                    alpha * 0.3,
                );
            }
            Id::Agent(..) => {
                let state = sim.state(id);
                let (n, s, w, _) = id.indices();
                g.line(
                    visible_position(sim, Id::Workspace(n, s, w)),
                    p,
                    [0.12, 0.3, 0.45],
                    alpha * 0.22,
                );
                g.glyph(Glyph::Agent(state), p, alpha, time, noise(id.seed()) * TAU);
            }
        }
    }
    for pulse in &sim.pulses {
        let age = (sim.clock - pulse.started) as f32 / PULSE_DURATION;
        let (n, s, w, _) = pulse.origin.indices();
        let start = visible_position(sim, pulse.origin);
        let session = visible_position(sim, Id::Session(n, s));
        let node = visible_position(sim, Id::Node(n));
        // Leaf -> workspace -> session ripple, then session -> node -> coordinator.
        let first_end = if matches!(pulse.origin, Id::Node(..)) {
            node
        } else {
            session
        };
        let p = if age < 0.35 {
            let local = ease(age / 0.35);
            let workspace = visible_position(sim, Id::Workspace(n, s, w));
            let ripple_center = if matches!(pulse.origin, Id::Agent(..)) {
                start
                    .lerp(workspace, local.min(0.5) * 2.0)
                    .lerp(session, (local - 0.5).max(0.0) * 2.0)
            } else {
                start.lerp(first_end, local)
            };
            for i in 0..36 {
                g.dot(
                    circle(ripple_center, 0.03 + local * 0.22, i as f32 * TAU / 36.0),
                    1.4,
                    pulse.color,
                    (1.0 - local) * 0.7,
                );
            }
            ripple_center
        } else if age < 0.55 {
            arc(first_end, node, ease((age - 0.35) / 0.2), 0.18)
        } else {
            arc(node, root, ease((age - 0.55) / 0.45), 0.5)
        };
        g.dot(p, 5.0, pulse.color, (PI * age).sin().max(0.0));
        if age > 0.55 {
            let head = ease((age - 0.55) / 0.45);
            for i in 1..12 {
                let t = (head - i as f32 * 0.012).max(0.0);
                g.dot(
                    arc(node, root, t, 0.5),
                    2.5,
                    pulse.color,
                    (1.0 - i as f32 / 12.0) * 0.45,
                );
            }
        }
    }
    debug_assert!(g.particles.len() <= MAX_PARTICLES);
    debug_assert!(g.lines.len() <= MAX_LINES);
    g
}
pub fn project(pos: Vec3, time: f32, rect: egui::Rect) -> Option<egui::Pos2> {
    let (vp, model, _) = camera(
        time,
        Viewport {
            x: rect.left(),
            y: rect.top(),
            width: rect.width(),
            height: rect.height(),
            pixels_per_point: 1.0,
        },
    );
    let clip = vp * model * pos.extend(1.0);
    if clip.w <= 0.0 {
        return None;
    }
    let p = clip.truncate() / clip.w;
    Some(egui::pos2(
        rect.center().x + p.x * rect.width() * 0.5,
        rect.center().y - p.y * rect.height() * 0.5,
    ))
}

// Grow cards to their text and stack independently in the two edge columns.
// The reserved margin also contains their +/-9 px animated drift.
fn callout_rect(
    rect: egui::Rect,
    bottom: f32,
    next_y: &mut [f32; 2],
    prefer_left: bool,
    height: f32,
) -> Option<(egui::Rect, bool)> {
    let width = 240.0_f32.min(rect.width() * 0.38).max(120.0);
    let left = prefer_left && rect.width() >= width * 2.0 + 48.0;
    let column = usize::from(left);
    let y = next_y[column];
    if y + height > bottom {
        return None;
    }
    next_y[column] += height + 28.0;
    let x = if left {
        rect.left() + 18.0
    } else {
        rect.right() - width - 18.0
    };
    Some((
        egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(width, height)),
        left,
    ))
}

pub fn overlay(ui: &egui::Ui, sim: &Simulation, rect: egui::Rect) {
    let painter = ui.painter().with_clip_rect(rect);
    let cyan = egui::Color32::from_rgb(90, 216, 235);
    painter.text(
        rect.left_top() + egui::vec2(20.0, 18.0),
        egui::Align2::LEFT_TOP,
        if sim.live {
            "HERDR MESH / LIVE OBSERVATIONS"
        } else {
            "HERDR MESH / LAST KNOWN OBSERVATIONS"
        },
        egui::FontId::monospace(12.0),
        cyan,
    );
    let stats = crate::mesh_stats::layout(&painter, rect, sim.summary());
    if let Some(anchor) = project(coordinator(sim.time), sim.time, rect) {
        painter.circle_stroke(
            anchor,
            14.0,
            egui::Stroke::new(1.0, egui::Color32::from_rgb(242, 177, 70)),
        );
    }
    let legend = crate::mesh_legend::layout(&painter, rect, stats.bounds.top() - 10.0);
    // Leave the key and footer readable even while event cards drift.
    let cards_top = rect.top() + 55.0;
    let cards_bottom = legend.bounds.top() - 20.0;
    let mut next_y = [cards_top; 2];
    if !sim.callouts || rect.width() < 260.0 || cards_bottom - cards_top < 84.0 {
        crate::mesh_legend::draw(&painter, &legend, sim.time);
        crate::mesh_stats::draw(&painter, &stats);
        return;
    }
    for event in sim.events.iter().rev() {
        let age = (sim.clock - event.started) as f32;
        let reveal = ease(age / CALLOUT_REVEAL);
        let opacity = callout_opacity(age);
        let col = egui::Color32::from_rgb(
            (event.color[0] * 255.0) as u8,
            (event.color[1] * 255.0) as u8,
            (event.color[2] * 255.0) as u8,
        )
        .gamma_multiply(opacity);
        let anchor =
            project(visible_position(sim, event.origin), sim.time, rect).unwrap_or(rect.center());
        let width = 240.0_f32.min(rect.width() * 0.38).max(120.0);
        let header = painter.layout(
            event.title.into(),
            egui::FontId::monospace(11.0),
            col,
            width - 24.0,
        );
        let galley = painter.layout(
            event.text.clone(),
            egui::FontId::monospace(10.0),
            col.gamma_multiply(0.8),
            width - 24.0,
        );
        let body_top = 12.0 + header.size().y + 10.0;
        let height = (body_top + galley.size().y + 24.0).max(84.0);
        let Some((card, left)) = callout_rect(
            rect,
            cards_bottom,
            &mut next_y,
            event.serial.is_multiple_of(2),
            height,
        ) else {
            continue;
        };
        // HUD cards drift gently at the edges while their leaders track real 3D anchors.
        let card = card.translate(egui::vec2(
            (1.0 - reveal) * 22.0,
            (sim.time * 0.24 + (event.serial % 3) as f32).sin() * 9.0,
        ));
        painter.rect_filled(
            card,
            0.0,
            egui::Color32::from_rgba_unmultiplied(4, 12, 24, (205.0 * opacity) as u8),
        );
        painter.rect_stroke(
            card,
            0.0,
            egui::Stroke::new(0.7, col.gamma_multiply(0.45)),
            egui::StrokeKind::Inside,
        );
        for (a, b) in [
            (card.left_top(), card.left_top() + egui::vec2(16.0, 0.0)),
            (card.left_top(), card.left_top() + egui::vec2(0.0, 10.0)),
            (
                card.right_bottom(),
                card.right_bottom() - egui::vec2(16.0, 0.0),
            ),
            (
                card.right_bottom(),
                card.right_bottom() - egui::vec2(0.0, 10.0),
            ),
        ] {
            painter.line_segment([a, b], egui::Stroke::new(1.5, col));
        }
        let end = if left {
            card.right_center()
        } else {
            card.left_center()
        };
        let elbow = egui::pos2((anchor.x + end.x) * 0.5, end.y);
        painter.add(egui::Shape::line(
            vec![anchor, elbow, end],
            egui::Stroke::new(0.8, col.gamma_multiply(0.6)),
        ));
        painter.circle_stroke(
            anchor,
            4.0 + 3.0 * (1.0 - reveal),
            egui::Stroke::new(1.0, col),
        );
        let text = event
            .title
            .chars()
            .take((age * 36.0) as usize)
            .collect::<String>();
        let heading = painter.layout(text, egui::FontId::monospace(11.0), col, width - 24.0);
        painter.galley(card.left_top() + egui::vec2(12.0, 12.0), heading, col);
        painter.galley(card.left_top() + egui::vec2(12.0, body_top), galley, col);
        painter.line_segment(
            [
                card.left_bottom() + egui::vec2(12.0, -10.0),
                card.left_bottom() + egui::vec2(12.0 + (width - 24.0) * reveal, -10.0),
            ],
            egui::Stroke::new(1.0, col),
        );
    }
    // Paint last so moving leader lines cannot obscure the key.
    crate::mesh_legend::draw(&painter, &legend, sim.time);
    crate::mesh_stats::draw(&painter, &stats);
}

#[cfg(test)]
mod dpi_tests {
    use super::*;

    #[test]
    fn agent_state_and_breathing_preserve_a_readable_core_size() {
        for state in [
            AgentState::Working,
            AgentState::Blocked,
            AgentState::Completed,
            AgentState::Idle,
            AgentState::Unknown,
        ] {
            for time in [0., 1., 3., 5., 10.] {
                let glyph = glyph_geometry(Glyph::Agent(state), time);
                assert_eq!(glyph.particles.len(), 2);
                assert_eq!(glyph.particles[0].position_size[3], 5.0);
                assert!(glyph.particles[1].position_size[3] > glyph.particles[0].position_size[3]);
                assert!(
                    glyph.particles[0].color_softness[..3]
                        .iter()
                        .any(|c| *c > 0.)
                );
            }
        }
        let preview = Viewport::physical(
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(180., 88.)),
            1.,
            180,
            88,
        )
        .unwrap();
        let (_, _, scale) = camera(0., preview);
        assert!(5.0 * scale * 2. >= 9.0);
    }

    #[test]
    fn session_rings_follow_the_surface_with_enlarged_radius() {
        for time in [0., 10., 30., 60., 100., 200.] {
            for node in 0..16 {
                let center = position(Id::Session(node, 0), time);
                let rotation = scene_rotation(time);
                let normal = rotation * center.normalize();
                let mut geometry = Geometry {
                    particles: Vec::new(),
                    lines: Vec::new(),
                };
                geometry.glyph(Glyph::Session, center, 1., time, 0.);
                assert_eq!(geometry.particles.len(), 50);
                for dot in geometry.particles[2..].as_chunks::<2>().0 {
                    let [x, y, z, _] = dot[0].position_size;
                    let p = Vec3::new(x, y, z);
                    let offset = rotation * (p - center);
                    assert!(offset.dot(normal).abs() < 0.00001);
                    assert!((offset.length() - 0.22).abs() < 0.00001);
                }
            }
        }
    }

    #[test]
    fn marker_sizes_and_projected_geometry_are_uniform_in_logical_points() {
        // Tiny preview, normal window, portrait and large fullscreen exercise
        // both marker size limits and the camera's aspect-dependent distance.
        for size in [(180., 88.), (1280., 720.), (480., 960.), (2560., 1440.)] {
            let rect = egui::Rect::from_min_size(egui::pos2(24., 40.), egui::vec2(size.0, size.1));
            let base = Viewport::physical(rect, 1., 4000, 4000).unwrap();
            let (base_camera, base_model, base_size) = camera(3.0, base);
            for dpi in [1.0, 1.25, 1.5, 2.0, 3.0, 4.0] {
                let viewport = Viewport::physical(rect, dpi, 12000, 12000).unwrap();
                let (vp, model, marker_pixels) = camera(3.0, viewport);
                assert!((marker_pixels / dpi - base_size).abs() < 0.00001);
                assert!(vp.abs_diff_eq(base_camera, 0.00001));
                assert!(model.abs_diff_eq(base_model, 0.00001));
                for glyph in [
                    Glyph::Coordinator,
                    Glyph::Node,
                    Glyph::Session,
                    Glyph::Workspace,
                    Glyph::Agent(AgentState::Working),
                    Glyph::Agent(AgentState::Blocked),
                    Glyph::Agent(AgentState::Completed),
                    Glyph::Agent(AgentState::Idle),
                    Glyph::Agent(AgentState::Unknown),
                ] {
                    for particle in glyph_geometry(glyph, 3.0).particles {
                        let logical_radius = particle.position_size[3] * marker_pixels / dpi;
                        let expected = particle.position_size[3] * base_size;
                        assert!((logical_radius - expected).abs() < 0.00001);
                    }
                }
                for id in [
                    Id::Node(0),
                    Id::Session(0, 1),
                    Id::Workspace(0, 1, 2),
                    Id::Agent(0, 1, 2, 3),
                ] {
                    let pos = position(id, 3.0);
                    let clip = vp * model * pos.extend(1.0);
                    let ndc = clip.truncate() / clip.w;
                    let physical = egui::pos2(
                        viewport.x + (ndc.x + 1.) * viewport.width / 2.,
                        viewport.y + (1. - ndc.y) * viewport.height / 2.,
                    );
                    let logical = egui::pos2(physical.x / dpi, physical.y / dpi);
                    assert!(logical.distance(project(pos, 3.0, rect).unwrap()) < 0.001);
                }
            }
        }
    }
}

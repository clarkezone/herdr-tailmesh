//! 3D constellation geometry and projected movie-style callouts.
use crate::mesh_model::{AgentState, Id, PULSE_DURATION, Simulation, ease, noise};
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
pub(crate) fn scene_rotation(time: f32) -> Quat {
    Quat::from_euler(glam::EulerRot::XYZ, time * 0.018, time * 0.048, 0.0)
}
pub fn camera(time: f32, viewport: Viewport) -> (Mat4, Mat4, f32) {
    camera_pose(crate::orb_focus::Pose::ambient(time), viewport)
}
pub fn camera_for(sim: &Simulation, viewport: Viewport) -> (Mat4, Mat4, f32) {
    match sim.camera {
        Some(pose) => camera_pose(pose, viewport),
        None => camera(sim.time, viewport),
    }
}
fn camera_pose(pose: crate::orb_focus::Pose, viewport: Viewport) -> (Mat4, Mat4, f32) {
    let aspect = viewport.width / viewport.height.max(1.0);
    let half = (45.0_f32.to_radians() * 0.5).tan();
    let z = camera_distance(viewport) / pose.zoom;
    let projection =
        glam::camera::rh::proj::directx::perspective(45_f32.to_radians(), aspect, 0.1, 1000.0);
    let view = glam::camera::rh::view::look_at_mat4(Vec3::new(0.0, 0.0, z), Vec3::ZERO, Vec3::Y);
    let model =
        Mat4::from_scale_rotation_translation(Vec3::splat(1.1), pose.rotation, -pose.center);
    // Keep markers in logical points; zoom expands cluster spacing, not tiny dots.
    let scale = (viewport.height / viewport.pixels_per_point * 1.1 / (2.0 * z * half) / 105.0)
        .clamp(0.9, 1.5 + 0.7 * (pose.zoom - 1.).clamp(0., 1.))
        * viewport.pixels_per_point;
    (projection * view, model, scale)
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
    let pos = position(id, sim.motion_time());
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
                orbit(ring, i as f32 * TAU / 120.0, sim.motion_time()),
                orbit(ring, (i + 1) as f32 * TAU / 120.0, sim.motion_time()),
                [0.21, 0.09, 0.5],
                0.6,
            );
        }
    }
    let root = coordinator(sim.motion_time());
    let blocked = sim.blocked_nodes();
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
                if let Some(strength) = blocked.get(&id.indices().0) {
                    let breathing = 0.35 + 0.25 * (time * TAU / 4.8).sin();
                    g.dot(
                        p,
                        8.0,
                        AgentState::Blocked.color(),
                        alpha * strength * breathing,
                    );
                }
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
    // One bounded circuit per fresh blocked node, independent of agent count.
    // The slow amber ripple conveys attention, never a receipt heartbeat.
    for (n, strength) in blocked {
        let center = crate::mesh_territory::anchor(n) * 1.94;
        let phase = (time / 4.8 + noise(n as u32)) % 1.;
        let color = AgentState::Blocked.color();
        for i in 0_usize..32 {
            let angle = i as f32 * TAU / 32.;
            let a = circle(center, 0.28 + phase * 0.5, angle);
            let b = circle(center, 0.28 + phase * 0.5, angle + TAU / 16.);
            // Sixteen circuit segments leave headroom for the full retained-exit
            // line budget; keep all thirty-two lamps for the visual scanner.
            if i.is_multiple_of(2) {
                g.line(a, b, color, strength * (1. - phase) * 0.28);
            }
            let light = 0.12 + 0.28 * (angle - time * 1.3).sin().max(0.);
            g.dot(circle(center, 0.65, angle), 1.3, color, strength * light);
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
#[cfg(test)]
pub fn project(pos: Vec3, time: f32, rect: egui::Rect) -> Option<egui::Pos2> {
    project_pose(pos, None, time, rect)
}
pub fn project_sim(pos: Vec3, sim: &Simulation, rect: egui::Rect) -> Option<egui::Pos2> {
    project_pose(pos, sim.camera, sim.time, rect)
}
fn project_pose(
    pos: Vec3,
    pose: Option<crate::orb_focus::Pose>,
    time: f32,
    rect: egui::Rect,
) -> Option<egui::Pos2> {
    let viewport = Viewport {
        x: rect.left(),
        y: rect.top(),
        width: rect.width(),
        height: rect.height(),
        pixels_per_point: 1.,
    };
    let (vp, model, _) = camera_pose(
        pose.unwrap_or_else(|| crate::orb_focus::Pose::ambient(time)),
        viewport,
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

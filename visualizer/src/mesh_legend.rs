use egui::{Color32, FontId, Painter, Rect, Stroke, pos2, vec2};

use crate::mesh_model::AgentState;
use crate::mesh_orb::{Glyph, glyph_geometry};
use std::f32::consts::TAU;

pub struct Entry {
    pub label: &'static str,
    pub glyph: Glyph,
}
impl Entry {
    pub fn color(&self) -> Color32 {
        let [r, g, b] = self.glyph.color();
        egui::Rgba::from_rgb(r, g, b).into()
    }
}

const HIERARCHY_COUNT: usize = 5;

pub const ENTRIES: [Entry; 9] = [
    Entry {
        label: "Coordinator",
        glyph: Glyph::Coordinator,
    },
    Entry {
        label: "Node · N",
        glyph: Glyph::Node,
    },
    Entry {
        label: "Session · S",
        glyph: Glyph::Session,
    },
    Entry {
        label: "Workspace · W",
        glyph: Glyph::Workspace,
    },
    Entry {
        label: "Agent",
        glyph: Glyph::Agent(AgentState::Working),
    },
    Entry {
        label: "Working · A",
        glyph: Glyph::Agent(AgentState::Working),
    },
    Entry {
        label: "Needs input",
        glyph: Glyph::Agent(AgentState::Blocked),
    },
    Entry {
        label: "Completed",
        glyph: Glyph::Agent(AgentState::Completed),
    },
    Entry {
        label: "Idle / unknown",
        glyph: Glyph::Agent(AgentState::Unknown),
    },
];

pub struct Layout {
    pub bounds: Rect,
    cells: Vec<Rect>,
    icon_width: f32,
}

// A vertical hierarchy with indented agent states and their ownership bracket.
pub fn layout(painter: &Painter, viewport: Rect, bottom: f32) -> Layout {
    let available = (bottom.min(viewport.bottom()) - viewport.top()).max(1.);
    let bounds = Rect::from_min_size(viewport.min, vec2(viewport.width(), available));
    let icon_width = 36.;
    let mut cells = Vec::with_capacity(ENTRIES.len());
    if bounds.width() >= 140. && available >= 180. {
        let scale = (available / 324.).min(1.);
        let mut y = bounds.top() + 4. * scale;
        for (i, entry) in ENTRIES.iter().enumerate() {
            let indent = if i < HIERARCHY_COUNT { 4. } else { 20. };
            let height = if i < HIERARCHY_COUNT { 36. } else { 26. } * scale;
            let text =
                painter.layout_no_wrap(entry.label.into(), FontId::monospace(10.), entry.color());
            cells.push(Rect::from_min_size(
                pos2(bounds.left() + indent, y),
                vec2(icon_width + text.size().x, height),
            ));
            y += height + 4. * scale;
        }
    }
    Layout {
        bounds,
        cells,
        icon_width,
    }
}

// Front-facing, magnified samples of the sphere's own particle/line geometry.
// Soft radial meshes approximate the WGSL core/halo profile in the egui HUD.
pub fn sample(painter: &Painter, glyph: Glyph, rect: Rect, time: f32) {
    let painter = painter.with_clip_rect(painter.clip_rect().intersect(rect));
    let center = rect.center();
    let scale = ((rect.height() - 4.0) / 48.0).clamp(0.1, 1.0);
    let world_scale = if matches!(glyph, Glyph::Workspace) {
        140.0
    } else {
        85.0
    } * scale;
    let geometry = glyph_geometry(glyph, time);
    let project = |p: [f32; 3]| center + vec2(p[0], -p[1]) * world_scale;
    for line in geometry.lines.as_chunks::<2>().0 {
        let [r, g, b, a] = line[0].color;
        let color: Color32 = egui::Rgba::from_rgba_premultiplied(r, g, b, a).into();
        painter.line_segment(
            [project(line[0].position), project(line[1].position)],
            Stroke::new(1.0, color),
        );
    }
    // Additive glows sit behind cores, as in the scene, without opaque icon fills.
    for particle in &geometry.particles {
        let [x, y, _, size] = particle.position_size;
        let [r, g, b, softness] = particle.color_softness;
        let radius = size
            * scale
            * if matches!(glyph, Glyph::Agent(_)) {
                1.3
            } else {
                1.0
            };
        let origin = project([x, y, 0.0]);
        let mut mesh = egui::Mesh::default();
        let rings = [0.0_f32, 0.32, 0.48, 0.65, 0.82, 1.0];
        const SEGMENTS: usize = 16;
        for distance in rings {
            let t = ((distance - 0.48) / 0.34).clamp(0.0, 1.0);
            let hard_circle = 1.0 - t * t * (3.0 - 2.0 * t);
            let halo = (1.0 - distance).powf(1.55);
            let strength =
                (hard_circle * softness + halo * (1.0 - softness) * 1.45).min(1.0) * 1.45;
            let color: Color32 = egui::Rgba::from_rgba_premultiplied(
                (r * strength).min(1.0),
                (g * strength).min(1.0),
                (b * strength).min(1.0),
                0.0,
            )
            .into();
            for i in 0..SEGMENTS {
                let angle = i as f32 * TAU / SEGMENTS as f32;
                mesh.colored_vertex(
                    origin + vec2(angle.cos(), angle.sin()) * radius * distance,
                    color,
                );
            }
        }
        for ring in 0..rings.len() - 1 {
            for i in 0..SEGMENTS {
                let a = (ring * SEGMENTS + i) as u32;
                let b = (ring * SEGMENTS + (i + 1) % SEGMENTS) as u32;
                let c = a + SEGMENTS as u32;
                let d = b + SEGMENTS as u32;
                mesh.add_triangle(a, b, c);
                mesh.add_triangle(b, d, c);
            }
        }
        painter.add(egui::Shape::mesh(mesh));
    }
    if matches!(glyph, Glyph::Coordinator) {
        // The scene also draws this thin screen-facing ring around its gold root.
        painter.circle_stroke(
            center,
            14.0 * scale,
            Stroke::new(0.8, Color32::from_rgb(242, 177, 70)),
        );
    }
}

pub fn draw(painter: &Painter, layout: &Layout, time: f32) {
    let bounds = layout.bounds;
    if layout.cells.is_empty() {
        let painter = painter.with_clip_rect(painter.clip_rect().intersect(bounds));
        painter.text(
            bounds.left_top(),
            egui::Align2::LEFT_TOP,
            "Enlarge view for key",
            FontId::monospace(10.0),
            Color32::from_gray(175),
        );
        return;
    }
    for pair in layout.cells[..HIERARCHY_COUNT].windows(2) {
        if pair[0].top() == pair[1].top() {
            let center = pos2(pair[0].right() + 5.0, pair[0].center().y);
            painter.add(egui::Shape::line(
                vec![
                    center + vec2(-1.5, -2.5),
                    center + vec2(1.5, 0.0),
                    center + vec2(-1.5, 2.5),
                ],
                Stroke::new(0.8, Color32::from_gray(110)),
            ));
        }
    }
    // Link Agent to every state, routing through the left gutter when rows wrap.
    let parent = layout.cells[HIERARCHY_COUNT - 1];
    let states = &layout.cells[HIERARCHY_COUNT..];
    let rail_x = bounds.left() + 12.0;
    let root = pos2(parent.left() + layout.icon_width * 0.5, parent.bottom());
    let joint_y = states[0].top() - 8.0;
    let stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(105, 178, 203, 180));
    painter.add(egui::Shape::line(
        vec![root, pos2(root.x, joint_y), pos2(rail_x, joint_y)],
        stroke,
    ));
    painter.line_segment(
        [
            pos2(rail_x, joint_y),
            pos2(rail_x, states.last().unwrap().top() - 8.0),
        ],
        stroke,
    );
    for state in states {
        let x = state.left() + layout.icon_width * 0.5;
        let y = state.top() - 8.0;
        painter.add(egui::Shape::line(
            vec![pos2(rail_x, y), pos2(x, y), pos2(x, state.top() + 6.0)],
            stroke,
        ));
    }
    for (entry, cell) in ENTRIES.iter().zip(&layout.cells) {
        sample(
            painter,
            entry.glyph,
            Rect::from_min_size(cell.min, vec2(layout.icon_width, cell.height())),
            time,
        );
        painter.text(
            cell.left_center() + vec2(layout.icon_width, 0.0),
            egui::Align2::LEFT_CENTER,
            entry.label,
            FontId::monospace(10.0),
            entry.color(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vertical_key_preserves_order_glyphs_and_indented_state_rows() {
        let context = egui::Context::default();
        let viewport = Rect::from_min_size(pos2(45., 30.), vec2(176., 248.));
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(viewport),
                ..Default::default()
            },
            |ui| {
                let key = layout(ui.painter(), viewport, viewport.bottom());
                assert_eq!(key.cells.len(), ENTRIES.len());
                for (i, cell) in key.cells.iter().enumerate() {
                    assert!(key.bounds.contains_rect(*cell));
                    assert!(key.cells[..i].iter().all(|other| !other.intersects(*cell)));
                    if i >= HIERARCHY_COUNT {
                        assert!(cell.left() > key.cells[0].left());
                    }
                }
                draw(ui.painter(), &key, 0.);
            },
        );
        output.textures_delta.clear();
    }
    #[test]
    fn undersized_views_show_a_bounded_hint_instead_of_overlapping_labels() {
        let context = egui::Context::default();
        let viewport = Rect::from_min_size(pos2(45., 30.), vec2(90., 120.));
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(viewport),
                ..Default::default()
            },
            |ui| {
                let key = layout(ui.painter(), viewport, viewport.bottom());
                assert!(viewport.contains_rect(key.bounds));
                assert!(key.cells.is_empty());
                draw(ui.painter(), &key, 0.);
            },
        );
        output.textures_delta.clear();
    }
}

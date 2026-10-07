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
        label: "Node",
        glyph: Glyph::Node,
    },
    Entry {
        label: "Session",
        glyph: Glyph::Session,
    },
    Entry {
        label: "Workspace",
        glyph: Glyph::Workspace,
    },
    Entry {
        label: "Agent",
        glyph: Glyph::Agent(AgentState::Working),
    },
    Entry {
        label: "Working",
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

// Keep hierarchy and agent states on separate rows; wrap each group on narrow windows.
pub fn layout(painter: &Painter, viewport: Rect, bottom: f32) -> Layout {
    let bottom = bottom.clamp(viewport.top(), viewport.bottom());
    let width = (viewport.width() - 40.0).max(1.0);
    let inner_width = (width - 16.0).max(1.0);
    let icon_width = if inner_width >= 360.0 && viewport.height() >= 280.0 {
        48.0
    } else {
        32.0
    };
    let row_height = icon_width - 4.0;
    let mut cells = Vec::with_capacity(ENTRIES.len());
    let (mut x, mut y) = (0.0, 0.0);
    for (i, entry) in ENTRIES.iter().enumerate() {
        let text =
            painter.layout_no_wrap(entry.label.into(), FontId::monospace(10.0), entry.color());
        let cell_width = (text.size().x + icon_width + 6.0).min(inner_width);
        if i == HIERARCHY_COUNT {
            x = 14.0;
            y += row_height + 16.0;
        } else if x > 0.0 && x + cell_width > inner_width {
            x = if i > HIERARCHY_COUNT { 14.0 } else { 0.0 };
            y += row_height + if i > HIERARCHY_COUNT { 12.0 } else { 4.0 };
        }
        cells.push(Rect::from_min_size(
            pos2(x, y),
            vec2(cell_width, row_height),
        ));
        x += cell_width + 10.0;
    }
    let height = y + row_height + 16.0;
    // Do not overlap the title/footer or squeeze labels on undersized views.
    if viewport.width() < 220.0 || height + 50.0 > bottom - viewport.top() {
        let margin = (viewport.width() * 0.05).min(20.0);
        let hint_height = viewport.height().min(16.0);
        return Layout {
            bounds: Rect::from_min_size(
                pos2(
                    viewport.left() + margin,
                    (bottom - hint_height).max(viewport.top()),
                ),
                vec2((viewport.width() - margin * 2.0).max(1.0), hint_height),
            ),
            cells: Vec::new(),
            icon_width: 0.0,
        };
    }
    let bounds = Rect::from_min_size(
        pos2(viewport.left() + 20.0, bottom - height),
        vec2(width, height),
    );
    for cell in &mut cells {
        *cell = cell.translate(bounds.min.to_vec2() + vec2(8.0, 8.0));
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
    painter.rect_filled(bounds, 4.0, Color32::from_rgba_unmultiplied(4, 12, 24, 230));
    painter.rect_stroke(
        bounds,
        4.0,
        Stroke::new(0.5, Color32::from_white_alpha(35)),
        egui::StrokeKind::Inside,
    );
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
    fn key_wraps_without_overlapping_or_leaving_the_viewport() {
        let context = egui::Context::default();
        for size in [vec2(280.0, 420.0), vec2(650.0, 460.0), vec2(1200.0, 200.0)] {
            let viewport = Rect::from_min_size(pos2(45.0, 30.0), size);
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(viewport),
                    ..Default::default()
                },
                |ui| {
                    let key = layout(ui.painter(), viewport, viewport.bottom() - 40.0);
                    let Layout {
                        bounds,
                        cells,
                        icon_width,
                    } = &key;
                    assert!(viewport.contains_rect(*bounds));
                    assert_eq!(cells.len(), ENTRIES.len());
                    for (i, cell) in cells.iter().enumerate() {
                        assert!(bounds.contains_rect(*cell));
                        let text = ui.painter().layout_no_wrap(
                            ENTRIES[i].label.into(),
                            FontId::monospace(10.0),
                            ENTRIES[i].color(),
                        );
                        assert!(cell.width() >= icon_width + text.size().x);
                        for other in &cells[i + 1..] {
                            assert!(!cell.intersects(*other));
                        }
                    }
                    assert!(cells[HIERARCHY_COUNT].top() > cells[HIERARCHY_COUNT - 1].top());
                    draw(ui.painter(), &key, 0.0);
                },
            );
            output.textures_delta.clear();
        }
    }

    #[test]
    fn undersized_views_show_a_bounded_hint_instead_of_overlapping_labels() {
        let context = egui::Context::default();
        for size in [vec2(90.0, 240.0), vec2(320.0, 120.0), vec2(280.0, 240.0)] {
            let viewport = Rect::from_min_size(pos2(45.0, 30.0), size);
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(viewport),
                    ..Default::default()
                },
                |ui| {
                    let key = layout(ui.painter(), viewport, viewport.bottom() - 40.0);
                    assert!(viewport.contains_rect(key.bounds));
                    assert!(key.cells.is_empty());
                    draw(ui.painter(), &key, 0.0);
                },
            );
            output.textures_delta.clear();
        }
    }
}

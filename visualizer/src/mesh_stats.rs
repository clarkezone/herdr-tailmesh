use std::sync::Arc;

use egui::{Color32, FontId, Galley, Painter, Pos2, Rect, pos2, vec2};

use crate::mesh_model::Summary;

const COLOR: Color32 = Color32::from_rgb(90, 216, 235);

pub struct Layout {
    pub bounds: Rect,
    items: Vec<(Pos2, Arc<Galley>)>,
}
impl Layout {
    pub fn translate(&mut self, delta: egui::Vec2) {
        self.bounds = self.bounds.translate(delta);
        for (position, _) in &mut self.items {
            *position += delta;
        }
    }
}
fn values(summary: Summary, compact: bool) -> [(usize, &'static str); 7] {
    let labels = if compact {
        ["N", "S", "W", "A", "WORK", "BLOCK", "DONE"]
    } else {
        [
            "NODES",
            "SESSIONS",
            "WORKSPACES",
            "AGENTS",
            "WORKING",
            "BLOCKED",
            "COMPLETE",
        ]
    };
    let counts = [
        summary.nodes,
        summary.sessions,
        summary.workspaces,
        summary.agents,
        summary.states[0],
        summary.states[1],
        summary.states[2],
    ];
    std::array::from_fn(|i| (counts[i], labels[i]))
}
pub fn preferred_width(painter: &Painter, summary: Summary) -> f32 {
    values(summary, false)
        .iter()
        .map(|(count, label)| {
            painter
                .layout_no_wrap(format!("{count} {label}"), FontId::monospace(11.), COLOR)
                .size()
                .x
        })
        .sum::<f32>()
        + 18. * 6.
}

// Keep each count and label together, wrapping whole entries above the bottom margin.
pub fn layout(painter: &Painter, viewport: Rect, summary: Summary) -> Layout {
    layout_inner(painter, viewport, summary, false)
}
pub fn compact_layout(painter: &Painter, viewport: Rect, summary: Summary) -> Layout {
    layout_inner(painter, viewport, summary, true)
}
fn layout_inner(painter: &Painter, viewport: Rect, summary: Summary, compact: bool) -> Layout {
    let margin = (viewport.width() * 0.05).min(20.0);
    let width = (viewport.width() - 2.0 * margin).max(1.0);
    let font = if compact { 8. } else { 11. };
    let values = values(summary, compact);
    let mut items = Vec::with_capacity(values.len());
    let (mut x, mut y, mut row_height) = (0.0_f32, 0.0_f32, 0.0_f32);
    for (count, label) in values {
        let text = format!("{count} {label}");
        let mut galley = painter.layout_no_wrap(text.clone(), FontId::monospace(font), COLOR);
        // Very narrow views still keep a complete entry inside the viewport.
        if galley.size().x > width {
            let font_size = font * width / galley.size().x;
            galley = painter.layout_no_wrap(text, FontId::monospace(font_size), COLOR);
        }
        if x > 0.0 && x + galley.size().x > width {
            x = 0.0;
            y += row_height + if compact { 3. } else { 6. };
            row_height = 0.0;
        }
        items.push((pos2(x, y), galley.clone()));
        row_height = row_height.max(galley.size().y);
        x += galley.size().x + if compact { 8. } else { 18. };
    }
    let height = y + row_height;
    let bounds = Rect::from_min_size(
        pos2(viewport.left() + margin, viewport.bottom() - 18.0 - height),
        vec2(width, height),
    );
    for (position, _) in &mut items {
        *position += bounds.min.to_vec2();
    }
    Layout { bounds, items }
}

pub fn draw(painter: &Painter, layout: &Layout) {
    for (position, galley) in &layout.items {
        painter.galley(*position, galley.clone(), COLOR);
    }
}

use std::sync::Arc;

use egui::{Color32, FontId, Galley, Painter, Pos2, Rect, pos2, vec2};

use crate::mesh_model::Summary;

const COLOR: Color32 = Color32::from_rgb(90, 216, 235);

pub struct Layout {
    pub bounds: Rect,
    items: Vec<(Pos2, Arc<Galley>)>,
}

// Keep each count and label together, wrapping whole entries above the bottom margin.
pub fn layout(painter: &Painter, viewport: Rect, summary: Summary) -> Layout {
    let margin = (viewport.width() * 0.05).min(20.0);
    let width = (viewport.width() - 2.0 * margin).max(1.0);
    let values = [
        (summary.nodes, "NODES"),
        (summary.sessions, "SESSIONS"),
        (summary.workspaces, "WORKSPACES"),
        (summary.agents, "AGENTS"),
        (summary.states[0], "WORKING"),
        (summary.states[1], "BLOCKED"),
        (summary.states[2], "COMPLETE"),
    ];
    let mut items = Vec::with_capacity(values.len());
    let (mut x, mut y, mut row_height) = (0.0_f32, 0.0_f32, 0.0_f32);
    for (count, label) in values {
        let text = format!("{count} {label}");
        let mut galley = painter.layout_no_wrap(text.clone(), FontId::monospace(11.0), COLOR);
        // Very narrow views still keep a complete entry inside the viewport.
        if galley.size().x > width {
            let font_size = 11.0 * width / galley.size().x;
            galley = painter.layout_no_wrap(text, FontId::monospace(font_size), COLOR);
        }
        if x > 0.0 && x + galley.size().x > width {
            x = 0.0;
            y += row_height + 6.0;
            row_height = 0.0;
        }
        items.push((pos2(x, y), galley.clone()));
        row_height = row_height.max(galley.size().y);
        x += galley.size().x + 18.0;
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

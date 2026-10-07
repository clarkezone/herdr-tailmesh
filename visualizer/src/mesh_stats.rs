use std::sync::Arc;

use egui::{Color32, FontId, Galley, Painter, Pos2, Rect, pos2, vec2};

use crate::mesh_model::Summary;

const COLOR: Color32 = Color32::from_rgb(90, 216, 235);

pub struct Layout {
    pub bounds: Rect,
    items: Vec<(Pos2, Arc<Galley>)>,
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
/// A narrow, vertically aligned list; tiny previews retain every total.
pub fn layout(painter: &Painter, viewport: Rect, summary: Summary) -> Layout {
    let compact = viewport.width() < 110. || viewport.height() < 80.;
    let rows = values(summary, compact);
    let row_height = (viewport.height() / rows.len() as f32).clamp(0., 20.);
    let font = (row_height * 0.7).min(11.);
    let mut items = Vec::with_capacity(7);
    for (index, (count, label)) in rows.into_iter().enumerate() {
        let text = format!("{count} {label}");
        let mut galley = painter.layout_no_wrap(text.clone(), FontId::monospace(font), COLOR);
        if galley.size().x > viewport.width() {
            galley = painter.layout_no_wrap(
                text,
                FontId::monospace(font * viewport.width() / galley.size().x),
                COLOR,
            );
        }
        items.push((
            pos2(viewport.left(), viewport.top() + index as f32 * row_height),
            galley,
        ));
    }
    Layout {
        bounds: Rect::from_min_size(viewport.min, vec2(viewport.width(), row_height * 7.)),
        items,
    }
}

pub fn draw(painter: &Painter, layout: &Layout) {
    let painter = painter.with_clip_rect(layout.bounds.intersect(painter.clip_rect()));
    for (position, galley) in &layout.items {
        painter.galley(*position, galley.clone(), COLOR);
    }
}

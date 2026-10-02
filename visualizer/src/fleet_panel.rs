use crate::animation::Motion;
use egui::{Color32, FontId, Pos2, Rect, Sense, Stroke, Vec2};
use herdr_mesh_visualizer::{client::View, projection::wall_now, summary::summary};

pub const LABELS: [&str; 7] = [
    "Connected nodes",
    "Fresh Herdr nodes",
    "Workspaces",
    "Total agents",
    "Agents working",
    "Agents blocked",
    "Agents done",
];
#[derive(Default)]
pub struct FleetPanel {
    cards: Vec<Motion>,
    allocation: Option<Motion>,
    origin: Option<Pos2>,
}
pub fn columns(width: f32) -> usize {
    ((width + 8.) / 138.).floor().clamp(1., LABELS.len() as f32) as usize
}
impl FleetPanel {
    pub fn draw(&mut self, ui: &mut egui::Ui, view: &View) {
        ui.label("Fleet pulse · unfiltered fleet · scoped inventory observations");
        let counts = view.scene.as_ref().map(|scene| summary(scene, wall_now()));
        let live = view.live && counts.is_some();
        let fresh = counts
            .as_ref()
            .map(|c| c.fresh.values())
            .unwrap_or_default();
        let known = counts
            .as_ref()
            .map(|c| c.known.values())
            .unwrap_or_default();
        let total = counts.as_ref().map_or(0, |c| c.total);
        let notes = [
            format!("{total} registered"),
            "Ready · received within 30s".into(),
            "On fresh nodes".into(),
            "All states · fresh nodes".into(),
            "In progress · fresh nodes".into(),
            "Needs attention · fresh nodes".into(),
            "Observed state · not task success".into(),
        ];
        let width = ui.available_width().max(1.);
        let cols = columns(width);
        let card_width = ((width - (cols - 1) as f32 * 8.) / cols as f32).max(1.);
        let padding = 12_f32.min(card_width * 0.1);
        let text_width = (card_width - 2. * padding).max(1.);
        let clock = ui.input(|i| i.time);
        let accents = [
            Color32::from_rgb(112, 135, 159),
            Color32::from_rgb(99, 216, 239),
            Color32::from_rgb(112, 135, 159),
            Color32::from_rgb(139, 188, 255),
            Color32::from_rgb(139, 188, 255),
            Color32::from_rgb(255, 171, 90),
            Color32::from_rgb(99, 216, 239),
        ];
        let text: Vec<_> = (0..LABELS.len())
            .map(|i| {
                let note = if live {
                    notes[i].clone()
                } else if counts.is_some() {
                    format!(
                        "{} last known{}",
                        known[i],
                        if i == 1 {
                            " · freshness unconfirmed"
                        } else {
                            ""
                        }
                    )
                } else {
                    "Awaiting first snapshot".into()
                };
                let value = if live {
                    fresh[i].to_string()
                } else {
                    "—".into()
                };
                let painter = ui.painter();
                [
                    painter.layout(
                        LABELS[i].into(),
                        FontId::proportional(12.),
                        Color32::from_rgb(188, 203, 218),
                        text_width,
                    ),
                    painter.layout(
                        value,
                        FontId::proportional(30.),
                        if live { accents[i] } else { Color32::GRAY },
                        text_width,
                    ),
                    painter.layout(
                        note,
                        FontId::proportional(11.),
                        Color32::from_gray(150),
                        text_width,
                    ),
                ]
            })
            .collect();
        let row_height = text
            .iter()
            .map(|lines| lines.iter().map(|g| g.size().y).sum::<f32>() + 2. * padding + 12.)
            .fold(0., f32::max);
        let content_height = LABELS.len().div_ceil(cols) as f32 * (row_height + 8.) - 8.;
        let available = ui.available_height().max(1.);
        let cap = (available * 0.33).max(row_height).min(available);
        let target = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, content_height.min(cap)));
        let allocation = self
            .allocation
            .get_or_insert_with(|| Motion::new(target, 1., clock));
        allocation.retarget(target, 1., clock);
        let height = allocation.sample(clock).0.height().min(cap).max(1.);
        if allocation.active(clock) {
            ui.ctx().request_repaint();
        }
        egui::ScrollArea::vertical()
            .id_salt("fleet-pulse")
            .max_height(height)
            .min_scrolled_height(height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let origin = ui.clip_rect().min;
                if let Some(previous) = self.origin {
                    for card in &mut self.cards {
                        card.rebase(previous - origin);
                    }
                }
                self.origin = Some(origin);
                for i in 0..LABELS.len() {
                    let target = Rect::from_min_size(
                        Pos2::new(
                            (i % cols) as f32 * (card_width + 8.),
                            (i / cols) as f32 * (row_height + 8.),
                        ),
                        Vec2::new(card_width, row_height),
                    );
                    if self.cards.len() <= i {
                        self.cards.push(Motion::new(target, 1., clock));
                    }
                    self.cards[i].retarget(target, 1., clock);
                }
                let extent = self
                    .cards
                    .iter()
                    .map(|c| c.sample(clock).0.max.y.max(c.target().max.y))
                    .fold(content_height, f32::max);
                let (content, _) = ui.allocate_exact_size(Vec2::new(width, extent), Sense::hover());
                for (i, card) in self.cards.iter().enumerate() {
                    let mut rect = card.sample(clock).0;
                    rect.min.x = rect.min.x.clamp(0., width);
                    rect.max.x = rect.max.x.clamp(rect.min.x, width);
                    let rect = rect.translate(content.min.to_vec2());
                    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
                    painter.rect_filled(rect, 9., Color32::from_rgb(19, 28, 41));
                    painter.rect_stroke(
                        rect,
                        9.,
                        Stroke::new(1., Color32::from_rgb(44, 57, 73)),
                        egui::StrokeKind::Inside,
                    );
                    painter.line_segment(
                        [
                            rect.min + Vec2::new(padding, 1.),
                            Pos2::new((rect.max.x - padding).max(rect.min.x), rect.min.y + 1.),
                        ],
                        Stroke::new(2., accents[i]),
                    );
                    let mut pos = rect.min + Vec2::splat(padding);
                    for line in &text[i] {
                        painter.galley(pos, line.clone(), Color32::WHITE);
                        pos.y += line.size().y + 6.;
                    }
                    if card.active(clock) {
                        ui.ctx().request_repaint();
                    }
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use herdr_mesh_visualizer::{
        projection::{Freshness, Scene},
        summary::{ContextCounts, Counts, NodeCounts},
    };
    use std::sync::Arc;
    fn texts(output: &egui::FullOutput) -> Vec<String> {
        output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(t) => Some(t.galley.text().into()),
                _ => None,
            })
            .collect()
    }
    #[test]
    fn waiting_empty_and_retained_values_are_distinct() {
        let context = egui::Context::default();
        let mut panel = FleetPanel::default();
        let mut view = View::default();
        let mut render = |view: &View| {
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(1500., 600.))),
                    ..Default::default()
                },
                |ui| panel.draw(ui, view),
            );
            let text = texts(&output);
            output.textures_delta.clear();
            text
        };
        let waiting = render(&view);
        assert_eq!(
            waiting.iter().filter(|s| s.as_str() == "—").count(),
            LABELS.len()
        );
        assert!(waiting.iter().any(|s| s == "Awaiting first snapshot"));
        view.scene = Some(Arc::new(Scene::default()));
        view.live = true;
        let empty = render(&view);
        assert_eq!(
            empty.iter().filter(|s| s.as_str() == "0").count(),
            LABELS.len()
        );
        view.scene = Some(Arc::new(Scene {
            counts: vec![NodeCounts {
                connected: true,
                contexts: vec![ContextCounts {
                    freshness: Freshness::Receipt(wall_now()),
                    inventory: Counts {
                        workspaces: 3,
                        agents: 9,
                        working: 2,
                        blocked: 1,
                        done: 1,
                        ..Default::default()
                    },
                }],
            }],
            ..Default::default()
        }));
        let populated = render(&view);
        let total = populated.iter().position(|s| s == "Total agents").unwrap();
        assert_eq!(populated[total + 1], "9");
        assert_eq!(populated[total + 2], "All states · fresh nodes");
        view.live = false;
        let retained = render(&view);
        assert_eq!(
            retained.iter().filter(|s| s.as_str() == "—").count(),
            LABELS.len()
        );
        assert!(retained.iter().any(|s| s == "3 last known"));
        let total = retained.iter().position(|s| s == "Total agents").unwrap();
        assert_eq!(retained[total + 1], "—");
        assert_eq!(retained[total + 2], "9 last known");
        for label in LABELS {
            assert!(retained.iter().any(|s| s == label));
        }
    }
    #[test]
    fn narrow_short_reflow_is_bounded_and_settles() {
        let context = egui::Context::default();
        let mut panel = FleetPanel::default();
        let view = View::default();
        for width in [1500., 480., 120., 800.] {
            for step in 0..30 {
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 240.))),
                    time: Some(context.input(|i| i.time) + 1. / 60.),
                    ..Default::default()
                };
                let mut output = context.run_ui(input, |ui| {
                    panel.draw(ui, &view);
                    assert!(
                        ui.min_rect().max.x <= width + 0.1,
                        "horizontal containment at {width}"
                    );
                    assert!(
                        ui.min_rect().max.y < 200.,
                        "summary must leave space for the tree"
                    );
                });
                output.textures_delta.clear();
                if step == 29 {
                    let time = context.input(|i| i.time);
                    assert!(!panel.allocation.as_ref().unwrap().active(time));
                    assert!(panel.cards.iter().all(|m| !m.active(time)));
                }
            }
        }
    }
}

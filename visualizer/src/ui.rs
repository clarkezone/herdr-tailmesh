use crate::animation::{DURATION, Motion, ease_in_out};
use egui::{Color32, Pos2, Rect, Sense, Stroke, Vec2};
use herdr_mesh_visualizer::{
    client::View,
    projection::{Branch, Key, Scene, now},
};
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

#[derive(Default)]
pub struct UiState {
    collapsed: HashSet<Key>,
    selected: Option<Key>,
    scroll_offset: f32,
    animated: HashMap<Key, AnimatedRow>,
    initialized: bool,
    details_key: Option<Key>,
    canvas_origin: Option<Pos2>,
}
struct AnimatedRow {
    motion: Motion,
    label: Arc<egui::Galley>,
    status: Arc<egui::Galley>,
    parent: Option<Key>,
    color: Color32,
    marker: &'static str,
    present: bool,
}
struct Row<'a> {
    branch: &'a Branch,
    depth: usize,
    parent: Option<usize>,
}
fn rows<'a>(
    branches: &'a [Branch],
    depth: usize,
    parent: Option<usize>,
    collapsed: &HashSet<Key>,
    out: &mut Vec<Row<'a>>,
) {
    for b in branches {
        let index = out.len();
        out.push(Row {
            branch: b,
            depth,
            parent,
        });
        if !collapsed.contains(&b.key) {
            rows(&b.children, depth + 1, Some(index), collapsed, out);
        }
    }
}
// Children share their parent's horizontal band rather than reserving a whole
// column. Deep/narrow trees reduce indentation while retaining a rightward step.
fn indent_for_width(width: f32, max_depth: usize) -> f32 {
    if max_depth == 0 {
        return 0.;
    }
    let padding = (width * 0.04).min(12.);
    let inner_width = (width - 2. * padding).max(0.);
    let leaf_width = 260_f32.min(inner_width * 0.65);
    ((inner_width - leaf_width) / max_depth as f32).clamp(0., 88.)
}
struct PositionedRow {
    rect: Rect,
    label: Arc<egui::Galley>,
    status: Arc<egui::Galley>,
}
fn layout_rows(
    rows: &[Row<'_>],
    width: f32,
    painter: &egui::Painter,
    live: bool,
    time: i64,
) -> Vec<PositionedRow> {
    let max_depth = rows.iter().map(|r| r.depth).max().unwrap_or(0);
    let indent = indent_for_width(width, max_depth);
    let padding = (width * 0.04).min(12.);
    let mut y = 16.;
    rows.iter()
        .map(|r| {
            let x = padding + r.depth as f32 * indent;
            let row_width = (width - padding - x).max(0.);
            let text_width = (row_width - 28.).max(1.);
            let label = painter.layout(
                row_label(r.branch),
                egui::FontId::proportional(15.),
                Color32::WHITE,
                text_width,
            );
            let status = painter.layout(
                format!(
                    "{} · {}",
                    r.branch.status,
                    if live {
                        r.branch.freshness.label(time)
                    } else {
                        "last known"
                    }
                ),
                egui::FontId::proportional(12.),
                Color32::WHITE,
                text_width,
            );
            let height = (label.size().y + 6. + status.size().y + 8.).max(44.);
            let rect = Rect::from_min_size(Pos2::new(x, y), Vec2::new(row_width, height));
            y = rect.max.y + 12.;
            PositionedRow {
                rect,
                label,
                status,
            }
        })
        .collect()
}

fn row_label(branch: &Branch) -> String {
    let title = format!("{}  {}", branch.kind, branch.label);
    match branch.project_id.as_deref() {
        Some("") => format!("{title}\nproject: unassigned / unresolved"),
        Some(id) => format!(
            "{title}\nproject: {}",
            herdr_mesh_visualizer::projection::label(id)
        ),
        None => title,
    }
}

fn find<'a>(branches: &'a [Branch], key: &Key) -> Option<&'a Branch> {
    for b in branches {
        if b.key == *key {
            return Some(b);
        }
        if let Some(found) = find(&b.children, key) {
            return Some(found);
        }
    }
    None
}
fn keys(branches: &[Branch], set: &mut HashSet<Key>) {
    for b in branches {
        set.insert(b.key.clone());
        keys(&b.children, set);
    }
}
fn color(b: &Branch, live: bool, time: i64) -> Color32 {
    if !live || b.freshness.label(time) != "live" {
        return Color32::from_rgb(142, 150, 168);
    }
    match b.status.as_str() {
        "blocked" => Color32::from_rgb(255, 171, 90),
        "working" => Color32::from_rgb(99, 216, 239),
        "offline" | "unavailable" => Color32::from_rgb(231, 111, 131),
        _ => Color32::from_rgb(165, 226, 182),
    }
}
impl UiState {
    pub fn draw(&mut self, ui: &mut egui::Ui, view: &View, port: u16) {
        let selection_before = self.selected.clone();
        // Register behind all content so entity clicks and scrollbar interactions
        // win, while empty canvas space and the outer margin can clear selection.
        let background = ui.interact(ui.max_rect(), ui.id().with("background"), Sense::click());
        let details = egui::Frame::NONE
            .inner_margin(16)
            .show(ui, |ui| {
                ui.set_min_size(ui.available_size());
                ui.set_clip_rect(ui.clip_rect().intersect(ui.max_rect()));
                self.draw_contents(ui, view, port)
            })
            .inner;
        if background.clicked()
            && !background
                .interact_pointer_pos()
                .is_some_and(|p| details.is_some_and(|rect| rect.contains(p)))
        {
            self.selected = None;
        }
        if self.selected != selection_before {
            ui.ctx().request_repaint();
        }
    }

    fn draw_contents(&mut self, ui: &mut egui::Ui, view: &View, port: u16) -> Option<Rect> {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        ui.heading("Herdr mesh");
        ui.label(format!(
            "127.0.0.1:{port} · {} · {}",
            if view.daemon.is_empty() {
                "daemon unknown"
            } else {
                &view.daemon
            },
            view.status
        ));
        let Some(scene) = &view.scene else {
            ui.add_space(40.);
            ui.label(
                "Waiting for observations. The window stays available while the daemon is offline.",
            );
            return None;
        };
        let time = now();
        let stale = scene
            .nodes
            .iter()
            .flat_map(|n| n.children.iter())
            .any(|s| s.freshness.label(time) != "live");
        let fresh_nodes = if view.live {
            scene
                .nodes
                .iter()
                .filter(|n| n.children.iter().any(|s| s.freshness.label(time) == "live"))
                .count()
        } else {
            0
        };
        ui.label(format!(
            "{} connected nodes · {} with fresh Herdr observations · {} observed agents · {} working · {} blocked{}",
            scene.connected,
            fresh_nodes,
            scene.agents,
            scene.working,
            scene.blocked,
            if !view.live || stale {
                " · counts include last-known observations"
            } else {
                ""
            }
        ));
        if let Some(received) = view.received {
            ui.small(format!("Last snapshot {}s ago · expand/collapse with the branch marker · select text for details · wheel to scroll · middle-drag to scroll vertically",received.elapsed().as_secs()));
        }
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(Color32::from_rgb(99, 216, 239), "● working");
            ui.colored_label(Color32::from_rgb(255, 171, 90), "● blocked");
            ui.colored_label(
                Color32::from_rgb(142, 150, 168),
                "● stale / offline / unknown",
            );
        });
        if !scene.projects.is_empty() {
            egui::CollapsingHeader::new(format!("Projects ({}) · reported workspace IDs", scene.projects.len()))
                .default_open(true)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical().id_salt("project-index").max_height(88.).show(ui, |ui| {
                        for project in &scene.projects {
                            ui.label(format!("{} · {} workspace observations · {} nodes",
                                herdr_mesh_visualizer::projection::label(&project.id), project.workspaces, project.nodes))
                                .on_hover_text("Grouped by the exact reported project ID, not verified Git identity. Counts include retained observations; workspace identity is scoped to node and session/incarnation.");
                        }
                    });
                });
        }
        let mut valid = HashSet::new();
        keys(&scene.nodes, &mut valid);
        self.collapsed.retain(|k| valid.contains(k));
        if self
            .selected
            .as_ref()
            .is_some_and(|key| !valid.contains(key))
        {
            self.selected = None;
        }
        if scene.nodes.is_empty() {
            ui.label("Mesh connected; no nodes observed.");
        }
        if scene.agents == 0 && !scene.nodes.is_empty() {
            ui.small("Nodes observed; no agents reported.");
        }
        ui.separator();
        if self.selected.is_some() {
            self.details_key = self.selected.clone();
        }
        let progress = ui.ctx().animate_bool_with_time_and_easing(
            ui.id().with("observation-visibility"),
            self.selected.is_some(),
            DURATION as f32,
            ease_in_out,
        );
        if progress <= 0. {
            self.details_key = None;
            self.draw_tree(ui, scene, view.live, time);
            return None;
        }
        // Allocate bounded regions explicitly: intermediate sidebar widths must
        // never let wrapped detail widgets push the tree beyond the window.
        if ui.available_width() >= 800. {
            let (region, _) = ui.allocate_exact_size(ui.available_size(), Sense::hover());
            let reserved = 304. * progress;
            let tree_rect =
                Rect::from_min_max(region.min, Pos2::new(region.max.x - reserved, region.max.y));
            let mut tree = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("tree-region")
                    .max_rect(tree_rect),
            );
            tree.set_clip_rect(tree_rect.intersect(ui.clip_rect()));
            self.draw_tree(&mut tree, scene, view.live, time);
            let details_rect = Rect::from_min_max(
                Pos2::new(tree_rect.max.x + 24. * progress, region.min.y),
                region.max,
            );
            // A full-width details surface slides in behind a narrowing clip.
            let full_rect = Rect::from_min_size(details_rect.min, Vec2::new(280., region.height()));
            let mut details = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("observation-region")
                    .max_rect(full_rect),
            );
            details.set_clip_rect(details_rect.intersect(ui.clip_rect()));
            details.set_opacity(progress);
            if self.selected.is_none() {
                details.disable();
            }
            details.heading("Observation");
            self.draw_details(&mut details, scene, view.live, time, region.height());
            Some(details_rect)
        } else {
            let (region, _) = ui.allocate_exact_size(
                Vec2::new(ui.available_width(), 180. * progress),
                Sense::hover(),
            );
            let full_rect = Rect::from_min_size(region.min, Vec2::new(region.width(), 180.));
            let mut details = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt("observation-region")
                    .max_rect(full_rect),
            );
            details.set_clip_rect(region.intersect(ui.clip_rect()));
            details.set_opacity(progress);
            if self.selected.is_none() {
                details.disable();
            }
            egui::CollapsingHeader::new("Observation")
                .default_open(true)
                .show(&mut details, |ui| {
                    self.draw_details(ui, scene, view.live, time, 120.)
                });
            ui.separator();
            self.draw_tree(ui, scene, view.live, time);
            Some(region)
        }
    }

    fn draw_tree(&mut self, ui: &mut egui::Ui, scene: &Scene, live: bool, time: i64) {
        let mut pan_delta = 0.;
        let output = egui::ScrollArea::vertical()
            .id_salt("mesh-canvas")
            .auto_shrink([false, false])
            .vertical_scroll_offset(self.scroll_offset)
            .show(ui, |ui| {
                // Header/index/pane reflow can move the entire viewport. Rebase
                // geometry before retargeting, but do not animate scroll input.
                let origin = ui.clip_rect().min;
                if let Some(previous) = self.canvas_origin {
                    let delta = previous - origin;
                    for row in self.animated.values_mut() {
                        row.motion.rebase(delta);
                    }
                }
                self.canvas_origin = Some(origin);
                let mut visible = vec![];
                rows(&scene.nodes, 0, None, &self.collapsed, &mut visible);
                let width = ui.available_width().max(0.);
                let layout = layout_rows(&visible, width, ui.painter(), live, time);
                let clock = ui.input(|i| i.time);
                let current: HashMap<_, _> = visible
                    .iter()
                    .enumerate()
                    .map(|(i, row)| (row.branch.key.clone(), i))
                    .collect();
                for (i, row) in visible.iter().enumerate() {
                    let placed = &layout[i];
                    let parent = row.parent.map(|p| visible[p].branch.key.clone());
                    let origin = row.parent.map_or(placed.rect, |p| {
                        let anchor = self
                            .animated
                            .get(&visible[p].branch.key)
                            .map_or(layout[p].rect, |row| row.motion.sample(clock).0);
                        Rect::from_min_size(anchor.min, placed.rect.size())
                    });
                    let entry = self
                        .animated
                        .entry(row.branch.key.clone())
                        .or_insert_with(|| AnimatedRow {
                            motion: Motion::new(
                                if self.initialized {
                                    origin
                                } else {
                                    placed.rect
                                },
                                if self.initialized { 0. } else { 1. },
                                clock,
                            ),
                            label: placed.label.clone(),
                            status: placed.status.clone(),
                            parent: parent.clone(),
                            color: color(row.branch, live, time),
                            marker: "·",
                            present: true,
                        });
                    entry.motion.retarget(placed.rect, 1., clock);
                    entry.label = placed.label.clone();
                    entry.status = placed.status.clone();
                    entry.parent = parent;
                    entry.color = color(row.branch, live, time);
                    entry.marker = if row.branch.children.is_empty() {
                        "·"
                    } else if self.collapsed.contains(&row.branch.key) {
                        "+"
                    } else {
                        "−"
                    };
                    entry.present = true;
                }
                // Exiting/collapsed rows fade towards their surviving ancestor.
                // They retain only render data, and cannot receive entity clicks.
                let exits: Vec<_> = self
                    .animated
                    .iter()
                    .filter(|(key, row)| !current.contains_key(*key) && row.present)
                    .map(|(key, row)| (key.clone(), row.parent.clone()))
                    .collect();
                for (key, mut parent) in exits {
                    let mut anchor = None;
                    while let Some(key) = parent {
                        if let Some(index) = current.get(&key) {
                            anchor = Some(layout[*index].rect.min);
                            break;
                        }
                        parent = self.animated.get(&key).and_then(|row| row.parent.clone());
                    }
                    let entry = self.animated.get_mut(&key).unwrap();
                    let target = entry.motion.target();
                    entry.motion.retarget(
                        Rect::from_min_size(anchor.unwrap_or(target.min), target.size()),
                        0.,
                        clock,
                    );
                    entry.present = false;
                }
                self.animated
                    .retain(|_, row| row.present || row.motion.active(clock));
                self.initialized = true;
                if self.animated.values().any(|row| row.motion.active(clock)) {
                    ui.ctx().request_repaint();
                }
                let height = self
                    .animated
                    .values()
                    .map(|row| {
                        row.motion.sample(clock).0.max.y.max(if row.present {
                            row.motion.target().max.y
                        } else {
                            0.
                        }) + 16.
                    })
                    .fold(0., f32::max);
                let response = ui.interact(
                    ui.clip_rect(),
                    ui.id().with("tree-background"),
                    Sense::click_and_drag(),
                );
                let (rect, _) = ui.allocate_exact_size(Vec2::new(width, height), Sense::hover());
                if response.dragged_by(egui::PointerButton::Middle) {
                    pan_delta = response.drag_delta().y;
                }
                let painter = ui.painter();
                let mut order: Vec<_> = self.animated.keys().collect();
                order.sort_by(|a, b| {
                    self.animated[*a]
                        .motion
                        .sample(clock)
                        .0
                        .min
                        .y
                        .total_cmp(&self.animated[*b].motion.sample(clock).0.min.y)
                        .then(a.cmp(b))
                });
                for key in order {
                    let placed = &self.animated[key];
                    let (mut animated_rect, alpha) = placed.motion.sample(clock);
                    // Immediate containment on window shrink; motion still uses
                    // the unmodified interpolated geometry for future retargets.
                    animated_rect.min.x = animated_rect.min.x.clamp(0., width);
                    animated_rect.max.x = animated_rect.max.x.clamp(animated_rect.min.x, width);
                    let item = animated_rect.translate(rect.min.to_vec2());
                    if alpha <= 0. || !ui.clip_rect().intersects(item) {
                        continue;
                    }
                    let pos = item.min;
                    let hit_rect = Rect::from_min_size(
                        pos,
                        Vec2::new(
                            (28. + placed.label.size().x.max(placed.status.size().x))
                                .min(item.width()),
                            item.height(),
                        ),
                    );
                    let c = placed.color.gamma_multiply(alpha);
                    if let Some(parent) = placed
                        .parent
                        .as_ref()
                        .and_then(|key| self.animated.get(key))
                    {
                        let parent_rect =
                            parent.motion.sample(clock).0.translate(rect.min.to_vec2());
                        let start = Pos2::new(parent_rect.min.x + 6., parent_rect.max.y - 4.);
                        let end = Pos2::new(pos.x + 6., pos.y + 10.);
                        let bend = Pos2::new(start.x, end.y);
                        let stroke = Stroke::new(1., Color32::from_gray(65).gamma_multiply(alpha));
                        painter.line_segment([start, bend], stroke);
                        painter.line_segment([bend, end], stroke);
                    }
                    if self.selected.as_ref() == Some(key) {
                        painter.rect_filled(
                            hit_rect,
                            5.,
                            Color32::from_rgb(29, 47, 62).gamma_multiply(alpha),
                        );
                    }
                    let clipped = painter.with_clip_rect(item.intersect(ui.clip_rect()));
                    clipped.text(
                        pos,
                        egui::Align2::LEFT_TOP,
                        placed.marker,
                        egui::FontId::monospace(16.),
                        c,
                    );
                    clipped.galley_with_override_text_color(
                        pos + Vec2::new(20., 0.),
                        placed.label.clone(),
                        c,
                    );
                    clipped.galley_with_override_text_color(
                        pos + Vec2::new(20., placed.label.size().y + 6.),
                        placed.status.clone(),
                        Color32::from_gray(160).gamma_multiply(alpha),
                    );
                    if let Some(index) = current.get(key) {
                        let branch = visible[*index].branch;
                        let hit = ui
                            .interact(hit_rect, egui::Id::new(key), Sense::click())
                            .on_hover_text(branch.details.join("\n"));
                        if hit.clicked() {
                            if hit
                                .interact_pointer_pos()
                                .is_some_and(|p| p.x < pos.x + 20.)
                                && !branch.children.is_empty()
                            {
                                if !self.collapsed.remove(key) {
                                    self.collapsed.insert(key.clone());
                                }
                                ui.ctx().request_repaint();
                            } else {
                                self.selected = Some(key.clone());
                            }
                        }
                    }
                }
                if response.clicked() {
                    self.selected = None;
                }
            });
        let maximum = (output.content_size.y - output.inner_rect.height()).max(0.);
        self.scroll_offset = (output.state.offset.y - pan_delta).clamp(0., maximum);
    }

    fn draw_details(&self, ui: &mut egui::Ui, scene: &Scene, live: bool, time: i64, height: f32) {
        if let Some(b) = self
            .details_key
            .as_ref()
            .and_then(|k| find(&scene.nodes, k))
        {
            ui.label(&b.label);
            ui.label(format!(
                "{} · {}",
                b.status,
                if live {
                    b.freshness.label(time)
                } else {
                    "last known"
                }
            ));
            egui::ScrollArea::vertical()
                .id_salt("details")
                .max_height(height.min(ui.available_height()).max(0.))
                .show(ui, |ui| {
                    for detail in &b.details {
                        ui.add(egui::Label::new(detail).selectable(true).wrap());
                    }
                });
        } else {
            ui.label(if self.selected.is_some() {
                "Selected entity is absent from this replacement snapshot."
            } else {
                "Select a node, session, workspace or agent."
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use herdr_mesh_visualizer::projection::Freshness;

    fn branch(depth: usize, path: &str) -> Branch {
        Branch {
            key: vec![path.into()],
            kind: ["node", "session", "workspace", "agent"][depth.min(3)],
            label: "A very long mesh entity name with Unicode — café and an_unbroken_identifier_012345678901234567890123456789".into(),
            project_id: (depth == 2).then(|| "a_long_reported_project_identifier_".repeat(8)),
            details: vec![format!("Identifier: {}", "long_identifier_".repeat(80))],
            status: "working".into(),
            freshness: Freshness::Receipt(now()),
            children: if depth == 0 {
                vec![]
            } else {
                vec![branch(depth - 1, &format!("{path}/child"))]
            },
        }
    }

    fn frame(
        context: &egui::Context,
        state: &mut UiState,
        view: &View,
        width: f32,
        events: Vec<egui::Event>,
    ) -> egui::FullOutput {
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 900.))),
            time: Some(context.input(|i| i.time) + 1. / 60.),
            events,
            ..Default::default()
        };
        let mut output = context.run_ui(input, |ui| state.draw(ui, view, 8790));
        output.textures_delta.clear();
        output
    }

    fn text_position(output: &egui::FullOutput, text: &str) -> Option<(Pos2, Rect)> {
        output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(t) if t.galley.text() == text => Some((t.pos, shape.clip_rect)),
            _ => None,
        })
    }

    fn click(
        context: &egui::Context,
        state: &mut UiState,
        view: &View,
        width: f32,
        pos: Pos2,
    ) -> egui::FullOutput {
        // Real press/release frames exercise egui's overlapping hit targets.
        frame(
            context,
            state,
            view,
            width,
            vec![egui::Event::PointerMoved(pos)],
        );
        for pressed in [true, false] {
            frame(
                context,
                state,
                view,
                width,
                vec![egui::Event::PointerButton {
                    pos,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::default(),
                }],
            );
        }
        for _ in 0..(DURATION * 120.).ceil() as usize + 4 {
            frame(context, state, view, width, vec![]);
        }
        frame(context, state, view, width, vec![])
    }

    #[test]
    fn selection_opens_details_and_blank_click_restores_tree_space() {
        for width in [1200., 480.] {
            let context = egui::Context::default();
            let mut state = UiState::default();
            let mut node = branch(1, "first");
            node.label = "first".into();
            node.details = vec!["Selectable observation details".into()];
            let view = View {
                scene: Some(Arc::new(Scene {
                    nodes: vec![node],
                    ..Default::default()
                })),
                live: true,
                ..Default::default()
            };

            let output = frame(&context, &mut state, &view, width, vec![]);
            assert!(text_position(&output, "Observation").is_none());
            let (heading, _) = text_position(&output, "Herdr mesh").unwrap();
            assert_eq!(heading, Pos2::new(16., 16.));
            let (unselected_pos, unselected_clip) =
                text_position(&output, "session  first").unwrap();

            let output = click(
                &context,
                &mut state,
                &view,
                width,
                unselected_pos + Vec2::splat(5.),
            );
            assert_eq!(state.selected, Some(vec!["first".into()]));
            assert!(text_position(&output, "Observation").is_some());
            let (details_pos, _) = text_position(&output, "first").unwrap();
            let (selected_pos, selected_clip) = text_position(&output, "session  first").unwrap();
            if width >= 800. {
                assert!(selected_clip.max.x < unselected_clip.max.x - 250.);
            } else {
                assert!(selected_pos.y > unselected_pos.y);
            }

            // Details remain available for reading/copying and branch expansion.
            click(
                &context,
                &mut state,
                &view,
                width,
                details_pos + Vec2::splat(5.),
            );
            assert_eq!(state.selected, Some(vec!["first".into()]));
            let output = click(
                &context,
                &mut state,
                &view,
                width,
                Pos2::new(selected_pos.x - 14., selected_pos.y + 5.),
            );
            assert!(state.collapsed.contains(&vec!["first".into()]));
            assert!(text_position(&output, "Observation").is_some());

            // The unused portion of a row is background, not an entity hit target.
            let output = click(
                &context,
                &mut state,
                &view,
                width,
                Pos2::new(selected_clip.max.x - 5., selected_pos.y + 5.),
            );
            assert!(state.selected.is_none(), "blank row click at width {width}");
            assert!(text_position(&output, "Observation").is_none());
            let (restored_pos, restored_clip) = text_position(&output, "session  first").unwrap();
            assert_eq!(restored_clip.max.x, unselected_clip.max.x);
            assert_eq!(restored_pos.y, unselected_pos.y);

            click(
                &context,
                &mut state,
                &view,
                width,
                restored_pos + Vec2::splat(5.),
            );
            assert!(state.selected.is_some());
            let output = click(&context, &mut state, &view, width, Pos2::new(5., 5.));
            assert!(state.selected.is_none());
            assert!(text_position(&output, "Observation").is_none());

            let (pos, _) = text_position(&output, "session  first").unwrap();
            click(&context, &mut state, &view, width, pos + Vec2::splat(5.));
            assert!(state.selected.is_some());
            let output = click(&context, &mut state, &view, width, Pos2::new(100., 850.));
            assert!(state.selected.is_none(), "blank space below a short tree");
            assert!(text_position(&output, "Observation").is_none());
        }
    }

    #[test]
    fn removed_entity_clears_selection_and_observation_pane() {
        let context = egui::Context::default();
        let mut state = UiState {
            selected: Some(vec!["removed".into()]),
            ..Default::default()
        };
        let view = View {
            scene: Some(Arc::new(Scene::default())),
            ..Default::default()
        };
        let output = frame(&context, &mut state, &view, 1200., vec![]);
        assert!(state.selected.is_none());
        assert!(text_position(&output, "Observation").is_none());
    }

    #[test]
    fn observation_scroll_viewport_reserves_heading_and_summary_space() {
        let context = egui::Context::default();
        let state = UiState {
            details_key: Some(vec!["selected".into()]),
            ..Default::default()
        };
        let mut node = branch(0, "selected");
        node.label = "Selected node".into();
        node.details = (0..40).map(|i| format!("Detail line {i}")).collect();
        let scene = Scene {
            nodes: vec![node],
            ..Default::default()
        };
        let input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(280., 180.))),
            ..Default::default()
        };
        let mut output = context.run_ui(input, |ui| {
            ui.heading("Observation");
            state.draw_details(ui, &scene, true, now(), 180.);
            assert!(
                ui.min_rect().max.y <= 180. + 0.01,
                "the scroll viewport must fit below the heading and status summary"
            );
        });
        output.textures_delta.clear();
    }

    #[test]
    fn replacement_and_collapse_animate_and_prune_exiting_rows() {
        let context = egui::Context::default();
        let mut state = UiState::default();
        let make_view = |nodes| View {
            scene: Some(Arc::new(Scene {
                nodes,
                ..Default::default()
            })),
            live: true,
            ..Default::default()
        };
        let view = make_view(vec![branch(1, "existing")]);
        frame(&context, &mut state, &view, 1200., vec![]);
        let key = vec!["existing".into()];
        let child = vec!["existing/child".into()];
        let start = state.animated[&key]
            .motion
            .sample(context.input(|i| i.time))
            .0;
        let view = make_view(vec![branch(0, "inserted"), branch(1, "existing")]);
        frame(&context, &mut state, &view, 1200., vec![]);
        let clock = context.input(|i| i.time);
        assert_eq!(state.animated[&key].motion.sample(clock).0, start);
        assert!(state.animated[&key].motion.target().min.y > start.min.y);
        assert_eq!(
            state.animated[&vec!["inserted".into()]]
                .motion
                .sample(clock)
                .1,
            0.
        );
        for _ in 0..8 {
            frame(&context, &mut state, &view, 1200., vec![]);
        }
        let mid = state.animated[&key]
            .motion
            .sample(context.input(|i| i.time))
            .0;
        assert!(mid.min.y > start.min.y && mid.min.y < state.animated[&key].motion.target().min.y);
        for _ in 0..(DURATION * 60.).ceil() as usize {
            frame(&context, &mut state, &view, 1200., vec![]);
        }
        assert!(
            state
                .animated
                .values()
                .all(|row| !row.motion.active(context.input(|i| i.time)))
        );
        state.collapsed.insert(key.clone());
        frame(&context, &mut state, &view, 1200., vec![]);
        assert!(!state.animated[&child].present);
        for _ in 0..8 {
            frame(&context, &mut state, &view, 1200., vec![]);
        }
        let alpha = state.animated[&child]
            .motion
            .sample(context.input(|i| i.time))
            .1;
        assert!(alpha > 0. && alpha < 1.);
        // Reopen mid-fade, without replacing the scoped render identity.
        state.collapsed.remove(&key);
        frame(&context, &mut state, &view, 1200., vec![]);
        assert!(state.animated[&child].present);
        for _ in 0..(DURATION * 60.).ceil() as usize + 4 {
            frame(&context, &mut state, &view, 1200., vec![]);
        }
        assert_eq!(
            state.animated[&child]
                .motion
                .sample(context.input(|i| i.time))
                .1,
            1.
        );
        let empty = make_view(vec![]);
        // A surviving root leaves while its removed sibling fades out.
        let remaining = make_view(vec![branch(0, "inserted")]);
        frame(&context, &mut state, &remaining, 1200., vec![]);
        assert!(!state.animated[&key].present);
        for _ in 0..(DURATION * 60.).ceil() as usize + 4 {
            frame(&context, &mut state, &remaining, 1200., vec![]);
        }
        assert!(!state.animated.contains_key(&key));
        frame(&context, &mut state, &empty, 1200., vec![]);
        for _ in 0..(DURATION * 60.).ceil() as usize + 4 {
            frame(&context, &mut state, &empty, 1200., vec![]);
        }
        assert!(state.animated.is_empty());
    }

    #[test]
    fn clicks_follow_moving_rows_and_cannot_select_fading_exits() {
        let context = egui::Context::default();
        let mut state = UiState::default();
        let node = |key: &str| {
            let mut b = branch(0, key);
            b.label = key.into();
            b
        };
        let make_view = |nodes| View {
            scene: Some(Arc::new(Scene {
                nodes,
                ..Default::default()
            })),
            live: true,
            ..Default::default()
        };
        let old = make_view(vec![node("existing")]);
        frame(&context, &mut state, &old, 1200., vec![]);
        let changed = make_view(vec![node("inserted"), node("existing")]);
        let output = frame(&context, &mut state, &changed, 1200., vec![]);
        let (pos, _) = text_position(&output, "node  existing").unwrap();
        click(&context, &mut state, &changed, 1200., pos + Vec2::splat(5.));
        assert_eq!(state.selected, Some(vec!["existing".into()]));
        let removed = make_view(vec![node("inserted")]);
        let output = frame(&context, &mut state, &removed, 1200., vec![]);
        assert!(state.selected.is_none());
        let (exit, _) = text_position(&output, "node  existing").unwrap();
        click(
            &context,
            &mut state,
            &removed,
            1200.,
            exit + Vec2::splat(5.),
        );
        assert!(
            state.selected.is_none(),
            "fading rows must not receive entity clicks"
        );
        assert!(!state.animated.contains_key(&vec!["existing".into()]));
    }

    #[test]
    fn stacked_tree_fits_viewport_without_overlapping_wrapped_rows() {
        for depth in [0, 4, 16] {
            let branches = vec![branch(depth, "first"), branch(depth, "second")];
            let mut visible = vec![];
            rows(&branches, 0, None, &HashSet::new(), &mut visible);
            for width in [120., 280., 480., 800., 1200., 2000.] {
                let context = egui::Context::default();
                let mut output = context.run_ui(egui::RawInput::default(), |ui| {
                    let layout = layout_rows(&visible, width, ui.painter(), true, now());
                    for (index, placed) in layout.iter().enumerate() {
                        assert!(placed.rect.is_finite());
                        assert!(placed.rect.min.x >= 0.);
                        assert!(placed.rect.max.x <= width + 0.01);
                        assert!(placed.label.size().x + 20. <= placed.rect.width() + 0.01);
                        assert!(placed.status.size().x + 20. <= placed.rect.width() + 0.01);
                        assert!(
                            placed.label.size().y + placed.status.size().y + 6.
                                <= placed.rect.height()
                        );
                        if index > 0 {
                            assert!(placed.rect.min.y > layout[index - 1].rect.max.y);
                        }
                        if let Some(parent) = visible[index].parent {
                            assert!(placed.rect.min.x > layout[parent].rect.min.x);
                            assert!(placed.rect.min.x < layout[parent].rect.max.x);
                        }
                    }
                    if width <= 480. {
                        assert!(layout.iter().any(|placed| placed.rect.height() > 44.));
                    }
                });
                output.textures_delta.clear();
            }
        }
    }

    #[test]
    fn whole_view_fits_width_and_preserves_selection_across_resizes() {
        let selected = vec!["first/child/child/child/child".into()];
        let mut state = UiState {
            selected: Some(selected.clone()),
            ..Default::default()
        };
        let view = View {
            scene: Some(Arc::new(Scene {
                nodes: vec![branch(4, "first"), branch(4, "second")],
                ..Default::default()
            })),
            live: true,
            received: Some(std::time::Instant::now()),
            ..Default::default()
        };
        let context = egui::Context::default();
        for width in [1200., 820., 800., 480., 280., 1200.] {
            // Multiple frames also cover scroll-bar appearance and cached layout.
            for _ in 0..3 {
                let input = egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, 900.))),
                    ..Default::default()
                };
                let mut output = context.run_ui(input, |ui| {
                    state.draw(ui, &view, 8790);
                    assert!(
                        ui.min_rect().max.x <= width + 0.01,
                        "content exceeded window: {:?} at width {width}",
                        ui.min_rect()
                    );
                });
                output.textures_delta.clear();
                assert_eq!(state.selected.as_ref(), Some(&selected));
            }
        }
    }
}

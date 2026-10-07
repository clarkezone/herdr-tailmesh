//! A clock-driven holographic reveal for the entire Orb key/status panel.
use egui::{Color32, Painter, Pos2, Rect, Stroke, pos2, vec2};

use crate::mesh_model::ease;

pub const INTRO_END: f64 = 60.;
const REVEAL: f64 = 1.8;
pub(crate) const RETRACT: f64 = 1.6;
const CYAN: Color32 = Color32::from_rgb(90, 216, 235);

#[derive(Clone, Copy)]
pub struct Reveal {
    progress: f32,
}
impl Reveal {
    pub fn new(progress: f32) -> Self {
        Self {
            progress: if progress.is_finite() {
                progress.clamp(0., 1.)
            } else {
                0.
            },
        }
    }
    pub fn amount(self) -> f32 {
        self.progress
    }
    pub fn event(age: f64, persistent: bool) -> Self {
        let enter = (age / REVEAL).clamp(0., 1.);
        let leave = if persistent {
            1.
        } else {
            ((crate::mesh_model::CALLOUT_DURATION - age) / RETRACT).clamp(0., 1.)
        };
        Self::new(
            enter.min(leave) as f32
                * if persistent {
                    1.
                } else {
                    crate::mesh_model::callout_opacity(age as f32)
                },
        )
    }
    pub fn active(self) -> bool {
        self.progress > 0.
    }
    pub fn interactive(self) -> bool {
        self.progress == 1.
    }
    pub fn opacity(self) -> f32 {
        stage(self.progress, 0.14, 0.72)
    }
    /// Expand a horizontal rail, then unfold upwards, without scaling glyphs/text.
    pub fn aperture(self, bounds: Rect) -> Rect {
        let width = bounds.width() * stage(self.progress, 0., 0.22);
        let height = bounds.height() * stage(self.progress, 0.14, 0.88);
        Rect::from_min_size(
            pos2(
                bounds.center().x - width * 0.5,
                bounds.bottom() - height.max(1.),
            ),
            vec2(width, height.max(1.)),
        )
        .intersect(bounds)
    }
    pub fn leader(self, painter: &Painter, anchor: Pos2, bounds: Rect, color: Color32) {
        if !self.active() {
            return;
        }
        let card = self.aperture(bounds);
        let end = if card.center().x < anchor.x {
            card.right_center()
        } else {
            card.left_center()
        };
        let elbow = pos2((anchor.x + end.x) * 0.5, end.y);
        let a = anchor.distance(elbow);
        let b = elbow.distance(end);
        let length = (a + b) * stage(self.progress, 0., 0.34);
        let tip = if length <= a && a > 0. {
            anchor.lerp(elbow, length / a)
        } else if b > 0. {
            elbow.lerp(end, ((length - a) / b).clamp(0., 1.))
        } else {
            end
        };
        let mut path = vec![anchor];
        if length > a {
            path.push(elbow);
        }
        path.push(tip);
        let alpha = stage(self.progress, 0., 0.14);
        let energy = (self.progress * std::f32::consts::PI).sin().max(0.);
        if energy > 0. {
            painter.add(egui::Shape::line(
                path.clone(),
                Stroke::new(4., color.gamma_multiply(0.12 * energy)),
            ));
        }
        painter.add(egui::Shape::line(
            path,
            Stroke::new(0.8, color.gamma_multiply(0.3 * alpha)),
        ));
        if energy > 0. {
            painter.circle_filled(tip, 2.5, Color32::WHITE.gamma_multiply(energy));
        }
    }
    /// A travelling laser edge, circuit ticks and a fading holographic scan texture.
    pub fn scan(self, painter: &Painter, bounds: Rect) {
        if !self.active() || self.interactive() {
            return;
        }
        let aperture = self.aperture(bounds);
        let painter = painter.with_clip_rect(aperture.intersect(painter.clip_rect()));
        let energy = (self.progress * std::f32::consts::PI).sin().max(0.);
        let y = aperture.top() + 1.;
        for (width, alpha) in [(12., 0.035), (6., 0.09), (2.4, 0.65)] {
            painter.line_segment(
                [pos2(aperture.left(), y), pos2(aperture.right(), y)],
                Stroke::new(width, CYAN.gamma_multiply(alpha * energy)),
            );
        }
        painter.line_segment(
            [pos2(aperture.left(), y), pos2(aperture.right(), y)],
            Stroke::new(0.8, Color32::WHITE.gamma_multiply(0.9 * energy)),
        );
        let shift = self.progress * 80.;
        for i in 0..24 {
            let x = bounds.left()
                + (i as f32 * bounds.width() / 24. + shift).rem_euclid(bounds.width().max(1.));
            let tail = 4. + (i % 4) as f32 * 3.;
            painter.line_segment(
                [pos2(x, y), pos2(x, y + tail)],
                Stroke::new(1., CYAN.gamma_multiply(0.32 * energy)),
            );
        }
        // A fixed bound keeps tiny and very large preview surfaces equally cheap.
        for row in 0..48 {
            let y = aperture.bottom() - row as f32 * 6.;
            if y < aperture.top() {
                break;
            }
            painter.line_segment(
                [pos2(aperture.left(), y), pos2(aperture.right(), y)],
                Stroke::new(0.5, CYAN.gamma_multiply(0.055 * energy)),
            );
        }
    }
}
fn stage(progress: f32, start: f32, end: f32) -> f32 {
    ease((progress - start) / (end - start))
}

/// Reversible visibility, independent of observed agent state or source changes.
pub struct Visibility {
    from: f32,
    pub shown: bool,
    since: f64,
}
impl Visibility {
    fn new(from: f32) -> Self {
        Self {
            from,
            shown: true,
            since: 0.,
        }
    }
    pub fn reveal(&self, clock: f64) -> Reveal {
        let t =
            ((clock - self.since) / if self.shown { REVEAL } else { RETRACT }).clamp(0., 1.) as f32;
        Reveal::new(if self.shown {
            self.from + (1. - self.from) * t
        } else {
            self.from * (1. - t)
        })
    }
    fn set(&mut self, shown: bool, clock: f64, delay: f64) {
        if self.shown != shown {
            self.from = self.reveal(clock).amount();
            self.shown = shown;
            self.since = clock + delay;
        }
    }
    pub fn occupies(&self, clock: f64) -> bool {
        self.shown || self.reveal(clock).active()
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Panel {
    Key,
    Counts,
}
pub struct Controls {
    pub key: Visibility,
    pub counts: Visibility,
    pub workers: Visibility,
    pub bottom: Panel,
    intro: bool,
}
impl Default for Controls {
    fn default() -> Self {
        Self {
            key: Visibility::new(0.),
            counts: Visibility::new(0.),
            workers: Visibility::new(1.),
            bottom: Panel::Key,
            intro: true,
        }
    }
}
impl Controls {
    pub fn update(&mut self, clock: f64) {
        if self.intro && clock >= INTRO_END - RETRACT {
            self.key.set(false, INTRO_END - RETRACT, 0.);
            self.intro = false;
        }
    }
    pub fn toggle(&mut self, panel: Panel, clock: f64) {
        self.update(clock);
        if panel == Panel::Key {
            self.intro = false;
        }
        let (incoming, other) = match panel {
            Panel::Key => (&mut self.key, &self.counts),
            Panel::Counts => (&mut self.counts, &self.key),
        };
        let opening = !incoming.shown;
        let mut delay = 0.;
        if opening && !incoming.reveal(clock).active() {
            self.bottom = panel;
            if other.occupies(clock) {
                delay = crate::animation::DURATION;
            }
        }
        incoming.set(opening, clock, delay);
    }
    pub fn toggle_workers(&mut self, clock: f64) {
        self.workers.set(!self.workers.shown, clock, 0.);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn intro_is_once_and_keyboard_overrides_it_indefinitely() {
        let mut controls = Controls::default();
        for time in [2., 30., 59.] {
            controls.update(time);
            assert!(controls.key.reveal(time).active());
        }
        for time in [60., 180., 10_000.] {
            controls.update(time);
            assert!(!controls.key.reveal(time).active());
            assert!(controls.counts.reveal(time).interactive());
        }
        controls.toggle(Panel::Key, 10_001.);
        controls.update(50_000.);
        assert!(controls.key.reveal(50_000.).interactive());
        controls.toggle(Panel::Key, 50_001.);
        assert!(!controls.key.reveal(50_003.).active());
        let mut early = Controls::default();
        early.toggle(Panel::Key, 20.);
        early.toggle(Panel::Key, 25.);
        early.update(60.);
        assert!(early.key.reveal(60.).interactive());
    }
    #[test]
    fn toggles_reverse_continuously_and_workers_are_independent() {
        let mut controls = Controls::default();
        controls.toggle(Panel::Counts, 2.);
        let before = controls.counts.reveal(2.8).amount();
        controls.toggle(Panel::Counts, 2.8);
        assert_eq!(controls.counts.reveal(2.8).amount(), before);
        controls.toggle_workers(3.);
        assert!(!controls.workers.reveal(5.).active());
        assert!(controls.key.reveal(5.).active());
        assert!(controls.counts.reveal(5.).active());
        controls.toggle_workers(10.);
        assert!(controls.workers.reveal(12.).interactive());
    }
    #[test]
    fn incoming_panel_waits_for_docking_and_events_retract_before_expiry() {
        let mut controls = Controls::default();
        controls.update(90.);
        controls.toggle(Panel::Key, 90.);
        assert!(!controls.key.reveal(90.2).active());
        assert!(controls.key.reveal(91.).active());
        assert!(Reveal::event(5., true).interactive());
        assert!(Reveal::event(5000., true).interactive());
        assert!(Reveal::event(9., false).active());
        assert!(!Reveal::event(10., false).active());
    }
    #[test]
    fn unfold_and_retract_are_continuous_bounded_and_preserve_logical_size() {
        for size in [vec2(1., 1.), vec2(180., 60.), vec2(190., 390.)] {
            let bounds = Rect::from_min_size(pos2(20., 30.), size);
            let mut area = 0.;
            for step in 0..=100 {
                let reveal = Reveal::new(step as f32 / 100.);
                let clip = reveal.aperture(bounds);
                assert!(bounds.contains_rect(clip));
                assert!(clip.is_finite());
                assert!(clip.area() >= area);
                assert!((0. ..=1.).contains(&reveal.opacity()));
                area = clip.area();
            }
            assert_eq!(Reveal::new(1.).aperture(bounds), bounds);
        }
    }
}

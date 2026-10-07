//! A clock-driven holographic reveal for the entire Orb key/status panel.
use egui::{Color32, Painter, Pos2, Rect, Stroke, pos2, vec2};

use crate::mesh_model::ease;

const PERIOD: f64 = 180.;
const VISIBLE: f64 = 60.;
const REVEAL: f64 = 1.8;
const RETRACT: f64 = 1.6;
const CYAN: Color32 = Color32::from_rgb(90, 216, 235);

#[derive(Clone, Copy)]
pub struct Reveal {
    progress: f32,
}
impl Reveal {
    pub fn at(clock: f64) -> Self {
        let phase = if clock.is_finite() {
            clock.max(0.).rem_euclid(PERIOD)
        } else {
            VISIBLE
        };
        let progress = if phase < REVEAL {
            (phase / REVEAL) as f32
        } else if phase < VISIBLE - RETRACT {
            1.
        } else if phase < VISIBLE {
            ((VISIBLE - phase) / RETRACT) as f32
        } else {
            0.
        };
        Self { progress }
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
    pub fn leader(self, painter: &Painter, anchor: Pos2, bounds: Rect) {
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
                Stroke::new(4., CYAN.gamma_multiply(0.12 * energy)),
            ));
        }
        painter.add(egui::Shape::line(
            path,
            Stroke::new(0.8, CYAN.gamma_multiply(0.3 * alpha)),
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sixty_second_window_includes_transitions_and_repeats_every_three_minutes() {
        for cycle in [0., 180., 360., 180. * 100_000.] {
            for phase in [0.01, 0.9, 1.8, 30., 58.4, 59.2, 59.999] {
                assert!(Reveal::at(cycle + phase).active());
            }
            for phase in [0., 60., 90., 179.999] {
                assert!(!Reveal::at(cycle + phase).active());
            }
            for phase in [1.8, 30., 58.4] {
                assert!(Reveal::at(cycle + phase).interactive());
            }
        }
        assert!(!Reveal::at(f64::NAN).active());
        assert!(!Reveal::at(f64::INFINITY).active());
    }
    #[test]
    fn unfold_and_retract_are_continuous_bounded_and_preserve_logical_size() {
        for size in [vec2(1., 1.), vec2(180., 60.), vec2(600., 204.)] {
            let bounds = Rect::from_min_size(pos2(20., 30.), size);
            let mut last_area = 0.;
            for step in 0..=100 {
                let reveal = Reveal::at(REVEAL * step as f64 / 100.);
                let clip = reveal.aperture(bounds);
                assert!(bounds.contains_rect(clip));
                assert!(clip.is_finite());
                assert!(clip.area() >= last_area);
                assert!((0. ..=1.).contains(&reveal.opacity()));
                last_area = clip.area();
                let closing = Reveal::at(VISIBLE - RETRACT * step as f64 / 100.);
                assert!(
                    (closing.aperture(bounds).area() - clip.area()).abs()
                        <= size.x * size.y * 0.00001 + 0.01
                );
            }
            assert_eq!(Reveal::at(2.).aperture(bounds), bounds);
        }
    }
}

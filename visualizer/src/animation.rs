//! Retargetable geometry/opacity transitions. No network or frame-rate assumptions.
use egui::{Rect, Vec2};

pub const DURATION: f64 = 0.4;

/// Symmetric easing with zero velocity and acceleration at both endpoints.
/// Shared by row geometry/opacity and observation-pane allocation.
pub fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * t * (t * (t * 6. - 15.) + 10.)
}

pub struct Motion {
    from: Rect,
    target: Rect,
    from_alpha: f32,
    alpha: f32,
    started: f64,
}
impl Motion {
    pub fn new(rect: Rect, alpha: f32, time: f64) -> Self {
        Self {
            from: rect,
            target: rect,
            from_alpha: alpha,
            alpha,
            started: time - DURATION,
        }
    }
    pub fn sample(&self, time: f64) -> (Rect, f32) {
        let t = ((time - self.started) / DURATION).clamp(0., 1.) as f32;
        let eased = ease_in_out(t);
        (
            Rect::from_min_max(
                self.from.min.lerp(self.target.min, eased),
                self.from.max.lerp(self.target.max, eased),
            ),
            self.from_alpha + (self.alpha - self.from_alpha) * eased,
        )
    }
    pub fn retarget(&mut self, rect: Rect, alpha: f32, time: f64) {
        // Repeated identical snapshots must not restart an in-flight transition.
        if rect == self.target && alpha == self.alpha {
            return;
        }
        let (current, opacity) = self.sample(time);
        self.from = current;
        self.from_alpha = opacity;
        self.target = rect;
        self.alpha = alpha;
        self.started = time;
    }
    pub fn active(&self, time: f64) -> bool {
        time - self.started < DURATION
    }
    pub fn target(&self) -> Rect {
        self.target
    }
    /// Change the viewport coordinate frame without changing screen position.
    pub fn rebase(&mut self, delta: Vec2) {
        self.from = self.from.translate(delta);
        self.target = self.target.translate(delta);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Pos2, Vec2};
    fn rect(x: f32) -> Rect {
        Rect::from_min_size(Pos2::new(x, x), Vec2::splat(30.))
    }
    #[test]
    fn retargeting_preserves_current_position_and_opacity_and_finishes() {
        let mut motion = Motion::new(rect(0.), 0., 0.);
        motion.retarget(rect(100.), 1., 0.);
        assert_eq!(motion.sample(0.), (rect(0.), 0.));
        let midway = motion.sample(DURATION / 2.);
        assert_eq!(midway, (rect(50.), 0.5));
        motion.retarget(rect(200.), 0., DURATION / 2.);
        assert_eq!(motion.sample(DURATION / 2.), midway);
        motion.retarget(rect(200.), 0., DURATION);
        assert_eq!(motion.sample(2. * DURATION), (rect(200.), 0.));
        assert!(!motion.active(2. * DURATION));
    }

    #[test]
    fn easing_has_a_slow_start_and_finish_and_is_symmetric() {
        assert_eq!(ease_in_out(0.), 0.);
        assert_eq!(ease_in_out(1.), 1.);
        assert_eq!(ease_in_out(0.5), 0.5);
        assert!(ease_in_out(0.25) < 0.25);
        assert!(ease_in_out(0.75) > 0.75);
        for step in 0..100 {
            let t = step as f32 / 100.;
            assert!(ease_in_out(t + 0.01) >= ease_in_out(t) - 0.000001);
            assert!((ease_in_out(t) + ease_in_out(1. - t) - 1.).abs() < 0.000002);
        }
    }
}

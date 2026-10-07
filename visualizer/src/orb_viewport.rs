#[derive(Clone, Copy)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// Physical pixels per logical display point, including egui zoom.
    pub pixels_per_point: f32,
}

impl Viewport {
    pub fn physical(rect: egui::Rect, scale: f32, width: u32, height: u32) -> Option<Self> {
        if !scale.is_finite() || scale <= 0.0 || !rect.is_finite() {
            return None;
        }
        let x = (rect.left() * scale).floor().clamp(0., width as f32);
        let y = (rect.top() * scale).floor().clamp(0., height as f32);
        let right = (rect.right() * scale).ceil().clamp(x, width as f32);
        let bottom = (rect.bottom() * scale).ceil().clamp(y, height as f32);
        (right > x && bottom > y).then_some(Self {
            x,
            y,
            width: right - x,
            height: bottom - y,
            pixels_per_point: scale,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fractional_dpi_viewports_round_outward_and_stay_on_the_surface() {
        let rect = egui::Rect::from_min_max(egui::pos2(-2.4, 3.2), egui::pos2(80.3, 60.7));
        for scale in [1.0, 1.25, 1.5, 2.0, 3.0] {
            let viewport = Viewport::physical(rect, scale, 100, 80).unwrap();
            assert_eq!(viewport.pixels_per_point, scale);
            assert_eq!(viewport.x, 0.0);
            assert_eq!(viewport.y, (rect.top() * scale).floor());
            assert_eq!(viewport.width, (rect.right() * scale).ceil().min(100.0));
            assert_eq!(
                viewport.height,
                (rect.bottom() * scale).ceil().min(80.0) - viewport.y
            );
            assert!(viewport.width > 0.0 && viewport.height > 0.0);
            assert!(viewport.x + viewport.width <= 100.0);
            assert!(viewport.y + viewport.height <= 80.0);
        }
        assert!(Viewport::physical(rect, 2.0, 0, 0).is_none());
        for scale in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(Viewport::physical(rect, scale, 100, 80).is_none());
        }
    }
}

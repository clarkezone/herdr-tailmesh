#[derive(Clone, Copy)]
pub struct Viewport {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Viewport {
    pub fn physical(rect: egui::Rect, scale: f32, width: u32, height: u32) -> Option<Self> {
        let x = (rect.left() * scale).floor().clamp(0., width as f32);
        let y = (rect.top() * scale).floor().clamp(0., height as f32);
        let right = (rect.right() * scale).ceil().clamp(x, width as f32);
        let bottom = (rect.bottom() * scale).ceil().clamp(y, height as f32);
        (right > x && bottom > y).then_some(Self {
            x,
            y,
            width: right - x,
            height: bottom - y,
        })
    }
}

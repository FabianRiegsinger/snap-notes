use crate::settings::HoverSettings;

use iced::Rectangle;

pub fn gaussian_scale(distance: f32, max_mag: f32, spread: f32) -> f32 {
    1.0 + max_mag * (-distance * distance / (spread * spread)).exp()
}

/// Dock magnification follows the cursor much faster (settles in ~0.15 s).
const MAGNIFICATION_STIFFNESS: f32 = 1100.0;
const SETTLE_THRESHOLD: f32 = 0.001;
const MAX_STEP: f32 = 1.0 / 240.0;

/// Critically damped spring: fast as the stiffness allows, without overshoot.
pub struct AnimationState {
    current: f32,
    target: f32,
    velocity: f32,
    stiffness: f32,
    damping: f32,
}

impl AnimationState {
    pub fn new(value: f32, stiffness: f32) -> Self {
        Self::with_stiffness(value, stiffness)
    }

    pub fn with_stiffness(value: f32, stiffness: f32) -> Self {
        Self {
            current: value,
            target: value,
            velocity: 0.0,
            stiffness,
            damping: 2.0 * stiffness.sqrt(),
        }
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        // Fixed small substeps keep stiff springs stable on slow frames.
        let steps = (dt / MAX_STEP).ceil().max(1.0);
        let step = dt / steps;
        for _ in 0..steps as usize {
            let accel =
                -self.stiffness * (self.current - self.target) - self.damping * self.velocity;
            self.velocity += accel * step;
            self.current += self.velocity * step;
        }
        let displacement = self.current - self.target;

        if displacement.abs() < SETTLE_THRESHOLD && self.velocity.abs() < SETTLE_THRESHOLD {
            self.current = self.target;
            self.velocity = 0.0;
            false
        } else {
            true
        }
    }

    pub fn value(&self) -> f32 {
        self.current
    }
}

pub struct MagnificationState {
    scales: Vec<AnimationState>,
}

impl MagnificationState {
    pub fn new() -> Self {
        Self { scales: Vec::new() }
    }

    pub fn sync_count(&mut self, count: usize) {
        while self.scales.len() < count {
            self.scales
                .push(AnimationState::with_stiffness(1.0, MAGNIFICATION_STIFFNESS));
        }
        self.scales.truncate(count);
    }

    pub fn update(
        &mut self,
        cursor_y: Option<f32>,
        bar_centers: &[f32],
        dt: f32,
        hover: &HoverSettings,
    ) -> bool {
        self.sync_count(bar_centers.len());
        let mut animating = false;
        for (i, center) in bar_centers.iter().enumerate() {
            let target = match cursor_y {
                Some(y) => gaussian_scale((y - center).abs(), hover.magnification, hover.spread),
                None => 1.0,
            };
            self.scales[i].set_target(target);
            if self.scales[i].tick(dt) {
                animating = true;
            }
        }
        animating
    }

    pub fn scale(&self, index: usize) -> f32 {
        self.scales.get(index).map(|s| s.value()).unwrap_or(1.0)
    }
}

const OPEN_SECS: f32 = 0.45;
const CLOSE_SECS: f32 = 0.34;
const PEEK_OPEN_SECS: f32 = 0.28;
const PEEK_CLOSE_SECS: f32 = 0.2;

/// Time-based progress between a docked bar (0) and its expanded form (1).
pub struct Morph {
    progress: f32,
    opening: bool,
    /// Durations at speed 1.
    base_open_secs: f32,
    base_close_secs: f32,
    speed: f32,
}

impl Morph {
    /// Timing for opening a full note; `speed` 2 runs twice as fast.
    pub fn new(speed: f32) -> Self {
        Self::with_durations(OPEN_SECS, CLOSE_SECS, speed)
    }

    /// Quicker timing for the hover peek.
    pub fn peek(speed: f32) -> Self {
        Self::with_durations(PEEK_OPEN_SECS, PEEK_CLOSE_SECS, speed)
    }

    fn with_durations(base_open_secs: f32, base_close_secs: f32, speed: f32) -> Self {
        Self {
            progress: 0.0,
            opening: false,
            base_open_secs,
            base_close_secs,
            speed,
        }
    }

    /// Changes the speed without disturbing a running morph.
    pub fn set_speed(&mut self, speed: f32) {
        self.speed = speed;
    }

    pub fn open(&mut self) {
        self.opening = true;
    }

    pub fn restart(&mut self) {
        self.progress = 0.0;
        self.opening = true;
    }

    pub fn close(&mut self) {
        self.opening = false;
    }

    pub fn is_opening(&self) -> bool {
        self.opening
    }

    pub fn is_closed(&self) -> bool {
        !self.opening && self.progress <= 0.0
    }

    pub fn progress(&self) -> f32 {
        self.progress
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        if self.opening {
            self.progress = (self.progress + dt * self.speed / self.base_open_secs).min(1.0);
            self.progress < 1.0
        } else {
            self.progress = (self.progress - dt * self.speed / self.base_close_secs).max(0.0);
            self.progress > 0.0
        }
    }
}

pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

pub fn ease_in_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

pub fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub struct MorphFrame {
    pub rect: Rectangle,
    pub content_alpha: f32,
}

/// Interpolates from the docked bar to the open note. Width leads and height
/// follows, so the note pulls out of the screen edge and then unfolds; the
/// content fades in once the shape has (almost) landed.
pub fn morph_frame(from: Rectangle, to: Rectangle, progress: f32) -> MorphFrame {
    let p = progress.clamp(0.0, 1.0);
    let wt = ease_out_cubic(p / 0.75);
    let ht = ease_in_out_cubic(p / 0.9);

    let right = lerp(from.x + from.width, to.x + to.width, wt);
    let width = lerp(from.width, to.width, wt);
    let center_y = lerp(from.y + from.height / 2.0, to.y + to.height / 2.0, ht);
    let height = lerp(from.height, to.height, ht);

    MorphFrame {
        rect: Rectangle {
            x: right - width,
            y: center_y - height / 2.0,
            width,
            height,
        },
        content_alpha: ease_out_cubic((p - 0.7) / 0.3),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gaussian_at_zero_distance() {
        let s = gaussian_scale(0.0, 4.0, 50.0);
        assert!((s - 5.0).abs() < 0.001);
    }

    #[test]
    fn gaussian_far_away_is_one() {
        let s = gaussian_scale(500.0, 4.0, 50.0);
        assert!((s - 1.0).abs() < 0.01);
    }

    #[test]
    fn animation_reaches_target() {
        let mut anim = AnimationState::new(0.0, 300.0);
        anim.set_target(1.0);
        for _ in 0..200 {
            anim.tick(1.0 / 60.0);
        }
        assert!((anim.value() - 1.0).abs() < 0.01);
    }

    #[test]
    fn animation_reversal_no_jump() {
        let mut anim = AnimationState::new(0.0, 300.0);
        anim.set_target(1.0);
        for _ in 0..5 {
            anim.tick(1.0 / 60.0);
        }
        let mid = anim.value();
        anim.set_target(0.0);
        anim.tick(1.0 / 60.0);
        // Momentum carries it on briefly, but without a jump...
        assert!((anim.value() - mid).abs() < 0.05);
        // ...and it is heading back within a few frames.
        for _ in 0..3 {
            anim.tick(1.0 / 60.0);
        }
        assert!(anim.value() < mid);
    }

    #[test]
    fn morph_runs_open_and_closed() {
        let mut m = Morph::new(1.0);
        assert!(m.is_closed());
        m.open();
        while m.tick(1.0 / 60.0) {}
        assert_eq!(m.progress(), 1.0);
        m.close();
        while m.tick(1.0 / 60.0) {}
        assert!(m.is_closed());
    }

    #[test]
    fn morph_speed_two_finishes_in_half_time() {
        let mut m = Morph::new(2.0);
        m.open();
        m.tick(OPEN_SECS / 2.0 + 1e-4);
        assert_eq!(m.progress(), 1.0);
    }

    #[test]
    fn set_speed_keeps_progress() {
        let mut m = Morph::new(1.0);
        m.open();
        m.tick(OPEN_SECS / 4.0);
        let before = m.progress();
        m.set_speed(2.0);
        assert_eq!(m.progress(), before);
        m.tick(OPEN_SECS / 4.0);
        assert!((m.progress() - (before + 0.5)).abs() < 1e-4);
    }

    #[test]
    fn magnification_uses_hover_settings() {
        let hover = HoverSettings {
            magnification: 2.0,
            ..HoverSettings::default()
        };
        let mut mag = MagnificationState::new();
        for _ in 0..200 {
            mag.update(Some(100.0), &[100.0], 1.0 / 60.0, &hover);
        }
        assert!((mag.scale(0) - 3.0).abs() < 0.01);
    }

    #[test]
    fn morph_frame_endpoints() {
        use iced::{Point, Size};
        let from = Rectangle::new(Point::new(594.0, 280.0), Size::new(6.0, 30.0));
        let to = Rectangle::new(Point::new(200.0, 140.0), Size::new(320.0, 320.0));

        let start = morph_frame(from, to, 0.0);
        assert_eq!(start.rect, from);
        assert_eq!(start.content_alpha, 0.0);

        let end = morph_frame(from, to, 1.0);
        assert!((end.rect.x - to.x).abs() < 0.01);
        assert!((end.rect.y - to.y).abs() < 0.01);
        assert!((end.rect.width - to.width).abs() < 0.01);
        assert!((end.rect.height - to.height).abs() < 0.01);
        assert_eq!(end.content_alpha, 1.0);
    }

    #[test]
    fn morph_frame_width_leads_height() {
        use iced::{Point, Size};
        let from = Rectangle::new(Point::new(594.0, 280.0), Size::new(6.0, 30.0));
        let to = Rectangle::new(Point::new(200.0, 140.0), Size::new(320.0, 320.0));
        let mid = morph_frame(from, to, 0.4);
        let w_frac = (mid.rect.width - from.width) / (to.width - from.width);
        let h_frac = (mid.rect.height - from.height) / (to.height - from.height);
        assert!(w_frac > h_frac);
        assert_eq!(mid.content_alpha, 0.0);
    }

    #[test]
    fn magnification_settles_quickly_without_overshoot() {
        let mut mag = MagnificationState::new();
        let centers = vec![100.0];
        let target = 1.0 + HoverSettings::default().magnification;
        let mut peak: f32 = 0.0;
        let mut frames_to_95 = None;
        for frame in 1..=120 {
            mag.update(Some(100.0), &centers, 1.0 / 60.0, &HoverSettings::default());
            let s = mag.scale(0);
            peak = peak.max(s);
            if frames_to_95.is_none() && s - 1.0 >= 0.95 * HoverSettings::default().magnification {
                frames_to_95 = Some(frame);
            }
        }
        // Reaches 95% within ~0.2 s at 60 fps and never overshoots.
        let frames = frames_to_95.expect("never reached 95%");
        assert!(frames <= 12, "took {frames} frames");
        assert!(peak <= target + 0.001);
    }

    #[test]
    fn magnification_is_stable_at_low_frame_rates() {
        let mut mag = MagnificationState::new();
        let centers = vec![100.0];
        for _ in 0..60 {
            mag.update(Some(100.0), &centers, 1.0 / 30.0, &HoverSettings::default());
        }
        assert!((mag.scale(0) - (1.0 + HoverSettings::default().magnification)).abs() < 0.01);
    }

    #[test]
    fn magnification_state_scales_near_cursor() {
        let mut mag = MagnificationState::new();
        let centers = vec![100.0, 200.0, 300.0];
        mag.update(Some(200.0), &centers, 1.0 / 60.0, &HoverSettings::default());
        for _ in 0..200 {
            mag.update(Some(200.0), &centers, 1.0 / 60.0, &HoverSettings::default());
        }
        assert!(mag.scale(1) > mag.scale(0));
        assert!(mag.scale(1) > mag.scale(2));
    }
}

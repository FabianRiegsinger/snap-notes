pub fn gaussian_scale(distance: f32, max_mag: f32, spread: f32) -> f32 {
    1.0 + max_mag * (-distance * distance / (spread * spread)).exp()
}

const STIFFNESS: f32 = 300.0;
const DAMPING: f32 = 34.64; // 2.0 * sqrt(300.0) ≈ 34.64 (critically damped)
const SETTLE_THRESHOLD: f32 = 0.001;

pub struct AnimationState {
    current: f32,
    target: f32,
    velocity: f32,
}

impl AnimationState {
    pub fn new(value: f32) -> Self {
        Self {
            current: value,
            target: value,
            velocity: 0.0,
        }
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        let displacement = self.current - self.target;
        let accel = -STIFFNESS * displacement - DAMPING * self.velocity;
        self.velocity += accel * dt;
        self.current += self.velocity * dt;

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

const MAX_MAG: f32 = 4.0;
const SPREAD: f32 = 60.0;

pub struct MagnificationState {
    scales: Vec<AnimationState>,
}

impl MagnificationState {
    pub fn new() -> Self {
        Self { scales: Vec::new() }
    }

    pub fn sync_count(&mut self, count: usize) {
        while self.scales.len() < count {
            self.scales.push(AnimationState::new(1.0));
        }
        self.scales.truncate(count);
    }

    pub fn update(&mut self, cursor_y: Option<f32>, bar_centers: &[f32], dt: f32) -> bool {
        self.sync_count(bar_centers.len());
        let mut animating = false;
        for (i, center) in bar_centers.iter().enumerate() {
            let target = match cursor_y {
                Some(y) => gaussian_scale((y - center).abs(), MAX_MAG, SPREAD),
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
        let mut anim = AnimationState::new(0.0);
        anim.set_target(1.0);
        for _ in 0..200 {
            anim.tick(1.0 / 60.0);
        }
        assert!((anim.value() - 1.0).abs() < 0.01);
    }

    #[test]
    fn animation_reversal_no_jump() {
        let mut anim = AnimationState::new(0.0);
        anim.set_target(1.0);
        for _ in 0..5 {
            anim.tick(1.0 / 60.0);
        }
        let mid = anim.value();
        anim.set_target(0.0);
        anim.tick(1.0 / 60.0);
        assert!(anim.value() < mid);
    }

    #[test]
    fn magnification_state_scales_near_cursor() {
        let mut mag = MagnificationState::new();
        let centers = vec![100.0, 200.0, 300.0];
        mag.update(Some(200.0), &centers, 1.0 / 60.0);
        for _ in 0..200 {
            mag.update(Some(200.0), &centers, 1.0 / 60.0);
        }
        assert!(mag.scale(1) > mag.scale(0));
        assert!(mag.scale(1) > mag.scale(2));
    }
}

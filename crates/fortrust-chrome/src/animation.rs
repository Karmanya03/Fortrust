pub struct Animated {
    current: f32,
    target: f32,
    speed: f32,
}

impl Animated {
    pub fn new(value: f32, speed: f32) -> Self {
        Self {
            current: value,
            target: value,
            speed,
        }
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    pub fn tick(&mut self, dt: f32) -> bool {
        let delta = self.target - self.current;
        if delta.abs() < 0.001 {
            self.current = self.target;
            return false;
        }
        // Custom cubic bezier approximation: ease-out quart
        let t = (1.0 - (-self.speed * dt).exp()).min(1.0);
        let eased = 1.0 - (1.0 - t).powf(3.5);
        self.current += delta * eased;
        true
    }

    pub fn value(&self) -> f32 {
        self.current
    }

    pub fn is_settled(&self) -> bool {
        (self.target - self.current).abs() < 0.001
    }
}

/// Critically-damped spring for buttery-smooth UI motion. Use when the natural
/// exponential easing of `Animated` feels too "stiff" — e.g. tooltips, panel
/// reveals, parallax effects. For most sidebar work the `Animated` ease is
/// fine; the spring is offered for cases where overshoot is desirable.
#[allow(dead_code)] // public API for future use (tooltips, modal sheet, etc.)
#[derive(Debug, Clone, Copy)]
pub struct Spring {
    current: f32,
    velocity: f32,
    target: f32,
    /// Angular frequency of the spring (rad/s). Higher = faster settle.
    /// 18.0 gives ~250ms settle time without overshoot.
    pub stiffness: f32,
    /// Critical damping ratio. 1.0 = no overshoot. 0.85 = slight overshoot.
    pub damping: f32,
}

impl Spring {
    #[allow(dead_code)]
    pub const fn new(value: f32) -> Self {
        Self {
            current: value,
            velocity: 0.0,
            target: value,
            stiffness: 18.0,
            damping: 1.0,
        }
    }

    pub const fn with_stiffness(mut self, stiffness: f32) -> Self {
        self.stiffness = stiffness;
        self
    }

    pub const fn with_damping(mut self, damping: f32) -> Self {
        self.damping = damping;
        self
    }

    pub fn set_target(&mut self, target: f32) {
        self.target = target;
    }

    /// Advance the simulation by `dt` seconds. Returns true while the spring
    /// is still moving, false once it has settled.
    pub fn tick(&mut self, dt: f32) -> bool {
        // Spring force: -k * (x - target). Damping: -c * v.
        // Semi-implicit Euler integration — stable for any reasonable dt.
        let dt = dt.min(0.1); // cap big frame deltas (e.g. when tab is hidden)
        let spring = -self.stiffness * (self.current - self.target);
        let damp = -self.damping * 2.0 * self.stiffness.sqrt() * self.velocity;
        let accel = spring + damp;
        self.velocity += accel * dt;
        self.current += self.velocity * dt;
        // Settled when both position and velocity are tiny.
        (self.current - self.target).abs() > 0.001 || self.velocity.abs() > 0.01
    }

    pub fn value(&self) -> f32 {
        self.current
    }

    pub fn is_settled(&self) -> bool {
        (self.current - self.target).abs() < 0.001 && self.velocity.abs() < 0.01
    }
}

#[cfg(test)]
mod spring_tests {
    use super::*;

    #[test]
    fn spring_reaches_target() {
        let mut s = Spring::new(0.0);
        s.set_target(100.0);
        let mut steps = 0;
        while s.tick(1.0 / 60.0) && steps < 600 {
            steps += 1;
        }
        assert!(s.is_settled());
        assert!((s.value() - 100.0).abs() < 0.5);
    }

    #[test]
    fn spring_critical_damping_has_no_overshoot() {
        let mut s = Spring::new(0.0).with_damping(1.0);
        s.set_target(100.0);
        let mut max_overshoot = f32::MIN;
        for _ in 0..600 {
            s.tick(1.0 / 60.0);
            if s.value() > max_overshoot {
                max_overshoot = s.value();
            }
        }
        // Critically damped springs should not exceed the target.
        assert!(max_overshoot <= 100.5, "max was {max_overshoot}");
    }

    #[test]
    fn spring_underdamped_overshoots() {
        let mut s = Spring::new(0.0).with_damping(0.4);
        s.set_target(100.0);
        let mut max_overshoot = f32::MIN;
        for _ in 0..1200 {
            s.tick(1.0 / 60.0);
            if s.value() > max_overshoot {
                max_overshoot = s.value();
            }
        }
        // Underdamped: should overshoot by at least 1%.
        assert!(max_overshoot > 101.0, "max was {max_overshoot}");
    }
}

#[allow(dead_code)]
pub struct TabAnimation {
    pub width: Animated,
    pub opacity: Animated,
    pub slide_y: Animated,
}

#[allow(dead_code)]
impl TabAnimation {
    pub fn opening() -> Self {
        let mut anim = Self {
            width: Animated::new(0.0, 10.0),
            opacity: Animated::new(0.0, 8.0),
            slide_y: Animated::new(12.0, 12.0),
        };
        anim.width.set_target(1.0);
        anim.opacity.set_target(1.0);
        anim.slide_y.set_target(0.0);
        anim
    }

    pub fn begin_close(&mut self) {
        self.width.set_target(0.0);
        self.opacity.set_target(0.0);
    }

    pub fn is_closed(&self) -> bool {
        self.width.is_settled() && self.width.value() < 0.01
    }

    pub fn tick(&mut self, dt: f32) {
        self.width.tick(dt);
        self.opacity.tick(dt);
        self.slide_y.tick(dt);
    }
}

pub struct SidebarAnimation {
    pub overlay_offset: Animated,
}

pub const SIDEBAR_COLLAPSED_W: f32 = 0.0;
pub const SIDEBAR_EXPANDED_W: f32 = 386.0;

impl SidebarAnimation {
    pub fn new() -> Self {
        Self {
            overlay_offset: Animated::new(SIDEBAR_COLLAPSED_W, 9.0),
        }
    }

    pub fn open(&mut self) {
        self.overlay_offset.set_target(SIDEBAR_EXPANDED_W);
    }

    pub fn close(&mut self) {
        self.overlay_offset.set_target(SIDEBAR_COLLAPSED_W);
    }

    pub fn toggle(&mut self) {
        if self.overlay_offset.value() < 1.0 {
            self.open();
        } else {
            self.close();
        }
    }

    pub fn is_open(&self) -> bool {
        self.overlay_offset.value() > 1.0
    }

    pub fn tick(&mut self, dt: f32) {
        self.overlay_offset.tick(dt);
    }

    pub fn current_offset(&self) -> f32 {
        self.overlay_offset.value()
    }

    pub fn current_width(&self) -> f32 {
        self.overlay_offset.value()
    }
}

impl Default for SidebarAnimation {
    fn default() -> Self {
        Self::new()
    }
}

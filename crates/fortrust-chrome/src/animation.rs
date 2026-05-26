/// A single animated value that smoothly interpolates toward a target.
/// Call `.tick(dt)` each frame, then use `.value()` as the current state.
pub struct Animated {
    current: f32,
    target: f32,
    speed: f32, // Higher = faster. Typical: 8.0 for snappy, 4.0 for smooth
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

    /// Advance the animation by `dt` seconds. Returns true if still animating.
    /// Use exponential decay (feel): current += (target - current) * (1 - e^(-speed*dt))
    pub fn tick(&mut self, dt: f32) -> bool {
        let delta = self.target - self.current;
        if delta.abs() < 0.001 {
            self.current = self.target;
            return false;
        }
        // Exponential ease-out: feels physical, no overshoot
        self.current += delta * (1.0 - (-self.speed * dt).exp());
        true
    }

    pub fn value(&self) -> f32 {
        self.current
    }

    pub fn is_settled(&self) -> bool {
        (self.target - self.current).abs() < 0.001
    }
}

/// Animates a tab's visual state (width, opacity, y-offset for slide-in)
#[allow(dead_code)]
pub struct TabAnimation {
    pub width: Animated,   // 0.0 → 1.0 (proportion of full tab width)
    pub opacity: Animated, // 0.0 → 1.0
    pub slide_y: Animated, // Pixels offset from final position (slide-in from below)
}

#[allow(dead_code)]
impl TabAnimation {
    /// Creates an animation in the "opening" state (starts hidden, animates to visible)
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

    /// Triggers the "closing" animation. Check `is_closed()` to know when to remove the tab.
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

/// Sidebar expand/collapse animation
pub struct SidebarAnimation {
    pub width: Animated, // Between SIDEBAR_COLLAPSED_W and SIDEBAR_EXPANDED_W
}

pub const SIDEBAR_COLLAPSED_W: f32 = 52.0;
pub const SIDEBAR_EXPANDED_W: f32 = 220.0;

impl SidebarAnimation {
    pub fn new() -> Self {
        Self {
            width: Animated::new(SIDEBAR_COLLAPSED_W, 9.0),
        }
    }

    pub fn expand(&mut self) {
        self.width.set_target(SIDEBAR_EXPANDED_W);
    }

    pub fn collapse(&mut self) {
        self.width.set_target(SIDEBAR_COLLAPSED_W);
    }

    pub fn tick(&mut self, dt: f32) {
        self.width.tick(dt);
    }

    pub fn current_width(&self) -> f32 {
        self.width.value()
    }

    pub fn label_opacity(&self) -> f32 {
        let t = (self.width.value() - SIDEBAR_COLLAPSED_W) / (SIDEBAR_EXPANDED_W - SIDEBAR_COLLAPSED_W);
        ((t - 0.7) / 0.3).clamp(0.0, 1.0)
    }
}

impl Default for SidebarAnimation {
    fn default() -> Self {
        Self::new()
    }
}


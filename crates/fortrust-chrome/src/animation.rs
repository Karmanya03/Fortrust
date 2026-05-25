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
    pub width: Animated,
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
        let t =
            (self.width.value() - SIDEBAR_COLLAPSED_W) / (SIDEBAR_EXPANDED_W - SIDEBAR_COLLAPSED_W);
        ((t - 0.7) / 0.3).clamp(0.0, 1.0)
    }
}

impl Default for SidebarAnimation {
    fn default() -> Self {
        Self::new()
    }
}

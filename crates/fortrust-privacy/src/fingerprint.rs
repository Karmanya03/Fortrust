use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use tracing::debug;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoiseStrategy {
    Additive,
    Xor,
    Rounding,
}

#[derive(Debug, Clone)]
pub struct CanvasNoise {
    pub enabled: bool,
    pub noise_level: u8,
    pub strategy: NoiseStrategy,
    seed: u64,
}

impl CanvasNoise {
    pub fn new(seed: u64) -> Self {
        Self {
            enabled: true,
            noise_level: 3,
            strategy: NoiseStrategy::Xor,
            seed,
        }
    }

    pub fn perturb_pixels(&self, pixels: &mut [u8]) {
        if !self.enabled || pixels.is_empty() {
            return;
        }

        let mut rng = StdRng::seed_from_u64(self.seed);
        for pixel in pixels.iter_mut() {
            let noise = match self.strategy {
                NoiseStrategy::Additive => {
                    let n: u8 = rng.gen_range(0..=self.noise_level);
                    pixel.wrapping_add(n)
                }
                NoiseStrategy::Xor => {
                    let n: u8 = rng.gen_range(0..=self.noise_level);
                    *pixel ^ n
                }
                NoiseStrategy::Rounding => *pixel & 0b1111_1100,
            };
            *pixel = noise;
        }
    }

    pub fn perturb_value(&self, value: f64) -> f64 {
        if !self.enabled {
            return value;
        }
        let mut rng = StdRng::seed_from_u64(self.seed);
        let noise = rng.gen_range(-0.0001..0.0001);
        value + noise
    }
}

#[derive(Debug, Clone)]
pub struct AudioNoise {
    pub enabled: bool,
    pub seed: u64,
}

impl AudioNoise {
    pub fn new(seed: u64) -> Self {
        Self {
            enabled: true,
            seed,
        }
    }

    pub fn perturb_sample(&self, sample: f64) -> f64 {
        if !self.enabled {
            return sample;
        }
        let mut rng = StdRng::seed_from_u64(self.seed);
        let noise = rng.gen_range(-0.00005..0.00005);
        sample + noise
    }
}

#[derive(Debug, Clone)]
pub struct FingerprintGuard {
    pub canvas: CanvasNoise,
    pub audio: AudioNoise,
    pub screen_resolution: Option<(u32, u32)>,
    pub timezone_override: Option<String>,
    pub user_agent_override: Option<String>,
    pub hardware_concurrency: u32,
    pub device_memory: f64,
    pub platform_override: Option<String>,
    seed: u64,
}

impl FingerprintGuard {
    pub fn new() -> Self {
        let seed = rand::random::<u64>();
        Self {
            canvas: CanvasNoise::new(seed),
            audio: AudioNoise::new(seed),
            screen_resolution: Some((1920, 1080)),
            timezone_override: None,
            user_agent_override: None,
            hardware_concurrency: 4,
            device_memory: 8.0,
            platform_override: Some("Win32".to_owned()),
            seed,
        }
    }

    pub fn with_seed(seed: u64) -> Self {
        Self {
            canvas: CanvasNoise::new(seed),
            audio: AudioNoise::new(seed),
            screen_resolution: Some((1920, 1080)),
            timezone_override: None,
            user_agent_override: None,
            hardware_concurrency: 4,
            device_memory: 8.0,
            platform_override: Some("Win32".to_owned()),
            seed,
        }
    }

    pub fn regenerate_seed(&mut self) {
        self.seed = rand::random::<u64>();
        self.canvas.seed = self.seed;
        self.audio.seed = self.seed;
        debug!("Fingerprint noise seed regenerated");
    }

    pub fn get_noisy_navigator_property(&self, property: &str) -> Option<String> {
        match property {
            "userAgent" | "appVersion" | "platform" => {
                Some(self.user_agent_override.clone().unwrap_or_else(|| {
                    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36".to_owned()
                }))
            }
            "hardwareConcurrency" => Some(self.hardware_concurrency.to_string()),
            "deviceMemory" => Some(self.device_memory.to_string()),
            "language" => Some("en-US".to_owned()),
            "languages" => Some("en-US,en".to_owned()),
            "doNotTrack" => Some("1".to_owned()),
            "cookieEnabled" => Some("true".to_owned()),
            "maxTouchPoints" => Some("0".to_owned()),
            "vendor" => Some("Google Inc.".to_owned()),
            "vendorSub" => Some(String::new()),
            "product" => Some("Gecko".to_owned()),
            "productSub" => Some("20100101".to_owned()),
            "appName" => Some("Netscape".to_owned()),
            "appCodeName" => Some("Mozilla".to_owned()),
            "oscpu" => Some("Windows NT 10.0".to_owned()),
            "webdriver" => Some("false".to_owned()),
            _ => None,
        }
    }

    pub fn get_screen_resolution(&self) -> (u32, u32) {
        self.screen_resolution.unwrap_or((1920, 1080))
    }

    pub fn get_timezone(&self) -> String {
        self.timezone_override
            .clone()
            .unwrap_or_else(|| "UTC".to_owned())
    }
}

impl Default for FingerprintGuard {
    fn default() -> Self {
        Self::new()
    }
}

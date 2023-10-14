use std::time::Instant;

pub struct FpsMeter {
    last_time: Instant,
    frames: u32,
    fps: u32,
}

impl FpsMeter {
    pub fn new() -> Self {
        Self {
            last_time: Instant::now(),
            frames: 0,
            fps: 0,
        }
    }

    pub fn tick(&mut self) {
        self.frames += 1;
        let now = Instant::now();
        let elapsed = now - self.last_time;
        if elapsed.as_secs() >= 1 {
            self.fps = self.frames;
            self.frames = 0;
            self.last_time = now;
        }
    }

    pub fn fps(&self) -> u32 {
        self.fps
    }
}

use std::time::Instant;

use bincode::{Decode, Encode};

#[derive(Encode, Decode, Debug, Clone)]
pub struct SimpleBar {
    name: String,
    id: String,
    current_size: u64,
    size: u64,
}

impl SimpleBar {
    pub fn new(name: String, id: String, size: u64) -> Self {
        SimpleBar {
            name,
            id,
            current_size: 0,
            size,
        }
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn inc(&mut self, delta: u64) {
        if self.current_size + delta <= self.size {
            self.current_size += delta;
        } else if self.current_size == self.size {
            return;
        } else {
            self.current_size = self.size;
        }
    }
    pub fn to_progress_bar(self) -> ProgressBar {
        ProgressBar {
            name: self.name,
            id: self.id,
            current_size: self.current_size,
            size: self.size,
            last_size: 0,
            last_time: Instant::now(),
            last_speed: 0,
        }
    }
}

#[derive(Clone)]
pub struct ProgressBar {
    name: String,
    id: String,
    current_size: u64,
    size: u64,
    last_size: u64,
    last_time: Instant,
    last_speed: u64,
}

impl ProgressBar {
    pub fn new(name: String, id: String, size: u64) -> Self {
        ProgressBar {
            name,
            id,
            current_size: 0,
            size,
            last_size: 0,
            last_speed: 0,
            last_time: Instant::now(),
        }
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn inc(&mut self, delta: u64) {
        if self.current_size + delta <= self.size {
            self.current_size += delta;
        } else if self.current_size == self.size {
            return;
        } else {
            self.current_size = self.size;
        }
    }
    pub fn inc_to_finished(&mut self) {
        self.current_size = self.size;
    }
    pub fn calculate_speed(&mut self) -> u64 {
        let now = Instant::now();
        let duration = now.duration_since(self.last_time);
        if duration.as_secs() < 1 {
            return self.last_speed;
        }
        self.last_time = now;
        let progress_size = self.current_size - self.last_size;
        self.last_size = self.current_size;
        self.last_speed =
            ((progress_size as f64 / (duration.as_millis() as f64 / 1000.0)) + 0.5) as u64;
        self.last_speed
    }
    pub fn pos(&self) -> u16 {
        (((self.current_size as f64 / self.size as f64) * 100.0) + 0.5) as u16
    }
}

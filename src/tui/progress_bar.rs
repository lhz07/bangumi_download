use std::{collections::HashMap, fmt, time::Instant};

use bincode::{Decode, Encode};

#[derive(Encode, Decode, Debug, Clone, Copy)]
pub struct ProgressState {
    pub id: u128,
    pub current_size: u64,
    pub current_speed: u64,
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct SimpleBar {
    name: String,
    current_size: u64,
    current_speed: u64,
    size: u64,
}

pub trait Inc: BasicBar {
    fn inc(&mut self, delta: u64) {
        if self.current_size() + delta <= self.size() {
            self.set_current_size(self.current_size() + delta);
        } else if self.current_size() == self.size() {
            return;
        } else {
            self.set_current_size(self.size());
        }
    }
}

pub trait BasicBar {
    fn size(&self) -> u64;
    fn is_finished(&self) -> bool;
    fn current_size(&self) -> u64;
    fn set_current_size(&mut self, current_size: u64);
}

impl BasicBar for SimpleBar {
    fn size(&self) -> u64 {
        self.size
    }
    fn is_finished(&self) -> bool {
        self.current_size == self.size
    }
    fn current_size(&self) -> u64 {
        self.current_size
    }
    fn set_current_size(&mut self, current_size: u64) {
        self.current_size = current_size;
    }
}

impl BasicBar for ProgressBar {
    fn size(&self) -> u64 {
        self.size
    }
    fn is_finished(&self) -> bool {
        self.current_size == self.size
    }
    fn current_size(&self) -> u64 {
        self.current_size
    }
    fn set_current_size(&mut self, current_size: u64) {
        self.current_size = current_size;
    }
}

impl<T: BasicBar> Inc for T {}

impl SimpleBar {
    pub fn new(name: String, size: u64) -> Self {
        SimpleBar {
            name,
            current_size: 0,
            current_speed: 0,
            size,
        }
    }
    pub fn pos(&self) -> u16 {
        (((self.current_size as f64 / self.size as f64) * 100.0) + 0.5) as u16
    }
    pub fn to_progress_bar(self) -> ProgressBar {
        ProgressBar {
            name: self.name,
            current_size: self.current_size,
            size: self.size,
            last_size: 0,
            last_time: Instant::now(),
            last_speed: 0,
        }
    }

    pub fn current_size_format(&self) -> Bytes {
        self.current_size.into()
    }

    pub fn size_format(&self) -> Bytes {
        self.size.into()
    }

    pub fn inc_to_finished(&mut self) {
        self.current_size = self.size;
    }

    pub fn current_speed(&self) -> Bytes {
        self.current_speed.into()
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn set_current_speed(&mut self, current_speed: u64) {
        self.current_speed = current_speed;
    }
}

#[derive(Clone)]
pub struct ProgressBar {
    name: String,
    current_size: u64,
    size: u64,
    last_size: u64,
    last_time: Instant,
    last_speed: u64,
}

impl ProgressBar {
    pub fn new(name: String, size: u64) -> Self {
        ProgressBar {
            name,
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

    pub fn progress_state(&mut self, id: u128) -> ProgressState {
        let current_speed = self.calculate_speed();
        ProgressState {
            id,
            current_size: self.current_size,
            current_speed,
        }
    }

    pub fn calculate_speed_const(&self) -> u64 {
        const FACTOR: f64 = 0.005;
        let now = Instant::now();
        let duration = now.duration_since(self.last_time);
        if duration.as_millis() < 500 {
            return self.last_speed;
        }
        let progress_size = self.current_size - self.last_size;
        let new_speed =
            ((progress_size as f64 / (duration.as_millis() as f64 / 1000.0)) + 0.5) as u64;
        (FACTOR * self.last_speed as f64 + (1.0 - FACTOR) * new_speed as f64) as u64
    }
    pub fn calculate_speed(&mut self) -> u64 {
        const FACTOR: f64 = 0.005;
        let now = Instant::now();
        let duration = now.duration_since(self.last_time);
        if duration.as_millis() < 500 {
            return self.last_speed;
        }
        self.last_time = now;
        let progress_size = self.current_size - self.last_size;
        self.last_size = self.current_size;
        let new_speed =
            ((progress_size as f64 / (duration.as_millis() as f64 / 1000.0)) + 0.5) as u64;
        self.last_speed =
            (FACTOR * self.last_speed as f64 + (1.0 - FACTOR) * new_speed as f64) as u64;

        self.last_speed
    }

    pub fn to_simple_bar(self) -> SimpleBar {
        let current_speed = self.calculate_speed_const();
        SimpleBar {
            name: self.name,
            current_size: self.current_size,
            current_speed,
            size: self.size,
        }
    }
}

pub trait SpeedSum {
    fn speed(&self) -> Bytes;
}

impl SpeedSum for ProgressSuit<SimpleBar> {
    fn speed(&self) -> Bytes {
        let mut sum = 0;
        for progress in self.iter() {
            sum += progress.current_speed;
        }
        sum.into()
    }
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct ProgressSuit<T> {
    list: Vec<u128>,
    state: HashMap<u128, T>,
}

impl<T> ProgressSuit<T> {
    pub fn new() -> Self {
        ProgressSuit {
            list: Vec::new(),
            state: HashMap::new(),
        }
    }
    pub fn add(&mut self, id: u128, bar: T) {
        self.list.push(id);
        self.state.insert(id, bar);
    }

    pub fn remove(&mut self, id: u128) -> Option<T> {
        self.list.retain(|i| i != &id);
        self.state.remove(&id)
    }

    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn is_empty(&self) -> bool {
        self.list.is_empty()
    }

    pub fn iter(&self) -> Iter<'_, T> {
        Iter::new(self.list.iter(), &self.state)
    }

    pub fn get_bar_mut(&mut self, id: u128) -> Option<&mut T> {
        self.state.get_mut(&id)
    }
    pub fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&T) -> bool,
    {
        self.retain_mut(|bar| f(bar));
    }
    pub fn retain_mut<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut T) -> bool,
    {
        let ProgressSuit { list, state } = self;
        list.retain_mut(|id| {
            let should_retain = f(state.get_mut(id).unwrap());
            if !should_retain {
                state.remove(id);
            }
            should_retain
        });
    }
}

impl ProgressSuit<ProgressBar> {
    pub fn to_simple_bars(self) -> ProgressSuit<SimpleBar> {
        let ProgressSuit { list, state } = self;
        let state = state
            .into_iter()
            .map(|(id, bar)| (id, bar.to_simple_bar()))
            .collect::<HashMap<_, _>>();
        ProgressSuit { list, state }
    }
    pub fn state(&mut self) -> Vec<ProgressState> {
        self.state
            .iter_mut()
            .map(|(id, bar)| bar.progress_state(*id))
            .collect::<Vec<_>>()
    }
}

pub struct Iter<'a, T> {
    list: core::slice::Iter<'a, u128>,
    state: &'a HashMap<u128, T>,
}

impl<'a, T> Iter<'a, T> {
    fn new(list: core::slice::Iter<'a, u128>, state: &'a HashMap<u128, T>) -> Self {
        Iter { list, state }
    }
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> {
        if let Some(id) = self.list.next() {
            Some(self.state.get(id).unwrap())
        } else {
            None
        }
    }
}

pub struct Bytes(u64);

impl Bytes {
    const BYTE: f64 = 1024.0;
    pub fn as_string(&self) -> String {
        let (value, unit) = self.format();
        format!("{:.2} {}", value, unit)
    }
    pub fn as_kilobytes(&self) -> f64 {
        self.0 as f64 / Self::BYTE
    }

    pub fn as_megabytes(&self) -> f64 {
        self.0 as f64 / Self::BYTE.powi(2)
    }

    pub fn as_gigabytes(&self) -> f64 {
        self.0 as f64 / Self::BYTE.powi(3)
    }

    pub fn as_terabytes(&self) -> f64 {
        self.0 as f64 / Self::BYTE.powi(4)
    }

    pub fn format(&self) -> (f64, &'static str) {
        const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
        let mut value = self.0 as f64;
        let mut index = 0;
        while value >= Self::BYTE && index < UNITS.len() - 1 {
            value /= Self::BYTE;
            index += 1;
        }
        (value, UNITS[index])
    }
}

impl From<u64> for Bytes {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl fmt::Display for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (value, unit) = self.format();
        write!(f, "{:.2} {}", value, unit)
    }
}

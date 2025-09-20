use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{StatefulWidget, Widget};
use std::time::{Duration, Instant};

pub struct LoadingWidget;

pub struct LoadingState {
    /// - interval time
    interval: Duration,
    instant: Instant,
    state: u8,
}

impl Default for LoadingState {
    fn default() -> Self {
        Self::new()
    }
}

impl LoadingState {
    pub fn new() -> Self {
        Self {
            interval: Duration::from_millis(50),
            instant: Instant::now(),
            state: 0,
        }
    }
    pub fn interval(mut self, interval: Duration) -> Self {
        self.interval = interval;
        self
    }
    pub fn next_state(&mut self) -> &str {
        const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        if self.instant.elapsed() >= self.interval {
            if self.state >= (FRAMES.len() - 1) as u8 {
                self.state = 0;
            } else {
                self.state += 1;
            }
            self.instant = Instant::now();
        }
        FRAMES[self.state as usize]
    }
}

impl StatefulWidget for LoadingWidget {
    type State = LoadingState;
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let frame = state.next_state();
        frame.render(area, buf);
    }
}

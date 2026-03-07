use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::{StatefulWidget, Widget};
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;

use crate::config_manager::SafeSend;
use crate::tui::animator::AniCmd;

pub struct LoadingWidget;

pub struct LoadingState {
    /// - interval time
    interval: Duration,
    instant: Instant,
    state: u8,
    animator_tx: UnboundedSender<AniCmd>,
}

impl LoadingState {
    pub fn new(animator_tx: UnboundedSender<AniCmd>) -> Self {
        animator_tx.send_msg(AniCmd::Start);
        Self {
            animator_tx,
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

impl Drop for LoadingState {
    fn drop(&mut self) {
        self.animator_tx.send_msg(AniCmd::Stop);
    }
}

impl StatefulWidget for LoadingWidget {
    type State = LoadingState;
    fn render(self, area: Rect, buf: &mut Buffer, state: &mut Self::State) {
        let frame = state.next_state();
        frame.render(area, buf);
    }
}

use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color as RColor, Stylize};
use ratatui::widgets::{Block, Clear, Paragraph, StatefulWidget, Widget, Wrap};
use std::ops::{Add, Mul, Sub};
use std::time::{Duration, Instant};

pub struct Notification {
    title: String,
    content: String,
    instant: Option<Instant>,
    duration: Duration,
    fade_duration: Duration,
    should_disappear: bool,
}

pub struct NotificationWidget;

impl Notification {
    pub fn new(title: String, content: String) -> Self {
        Self {
            title,
            content,
            instant: None,
            duration: Duration::from_secs(5),
            fade_duration: Duration::from_secs(1),
            should_disappear: false,
        }
    }
    pub fn duration(mut self, duration: Duration) -> Self {
        assert!(duration >= self.fade_duration);
        self.duration = duration;
        self
    }
    pub fn should_disappear(&self) -> bool {
        self.should_disappear
    }
}
#[derive(Clone, Copy)]
struct Color(u8, u8, u8);
struct TColor(i16, i16, i16);

impl From<Color> for TColor {
    fn from(value: Color) -> Self {
        let Color(r1, g1, b1) = value;
        TColor(r1 as i16, g1 as i16, b1 as i16)
    }
}

impl From<Color> for RColor {
    fn from(value: Color) -> Self {
        let Color(r1, g1, b1) = value;
        RColor::Rgb(r1, g1, b1)
    }
}

impl From<TColor> for Color {
    fn from(value: TColor) -> Self {
        let TColor(r1, g1, b1) = value;
        Color(r1 as u8, g1 as u8, b1 as u8)
    }
}

impl Add for TColor {
    type Output = TColor;
    fn add(self, rhs: Self) -> Self::Output {
        let TColor(r1, g1, b1) = self;
        let TColor(r2, g2, b2) = rhs;
        TColor(r1 + r2, g1 + g2, b1 + b2)
    }
}

impl Sub for TColor {
    type Output = TColor;
    fn sub(self, rhs: Self) -> Self::Output {
        let TColor(r1, g1, b1) = self;
        let TColor(r2, g2, b2) = rhs;
        TColor(r1 - r2, g1 - g2, b1 - b2)
    }
}

impl Mul<f64> for TColor {
    type Output = TColor;
    fn mul(self, rhs: f64) -> Self::Output {
        let TColor(r1, g1, b1) = self;
        TColor(
            (r1 as f64 * rhs) as i16,
            (g1 as f64 * rhs) as i16,
            (b1 as f64 * rhs) as i16,
        )
    }
}

fn color_trans(from: Color, to: Color, alpha: f64) -> Color {
    debug_assert!((0.0..=1.0).contains(&alpha));
    let trans = TColor::from(to) - from.into();
    let new = TColor::from(from) + trans * alpha;
    new.into()
}

impl StatefulWidget for NotificationWidget {
    type State = Notification;
    fn render(
        self,
        area: ratatui::prelude::Rect,
        buf: &mut ratatui::prelude::Buffer,
        state: &mut Self::State,
    ) {
        let vertical =
            Layout::vertical([Constraint::Fill(1), Constraint::Percentage(30)]).split(area)[1];
        let area = Layout::horizontal([Constraint::Fill(1), Constraint::Percentage(40)])
            .split(vertical)[1];
        let block = Block::bordered().title(state.title.as_str());
        // let color = Color(20, 206, 247);
        let color = Color(255, 255, 255);
        let fg = match state.instant {
            Some(instant) => {
                let age = instant.elapsed();
                let show_duration = state.duration - state.fade_duration;
                if age > state.duration {
                    state.should_disappear = true;
                }
                let alpha = ((age.as_secs_f64() - show_duration.as_secs_f64()).max(0.0)
                    / state.fade_duration.as_secs_f64())
                .clamp(0.0, 1.0);
                if alpha > 0.0 {
                    let alpha = alpha.powi(2);
                    color_trans(color, Color(55, 55, 55), alpha)
                } else {
                    color // show normal
                }
            }
            None => {
                state.instant = Some(Instant::now());
                color // new notification
            }
        };
        // clear the area to ensure we are on the top
        let clear = Clear;
        clear.render(area, buf);
        let para = Paragraph::new(state.content.as_str())
            .block(block)
            .wrap(Wrap { trim: true })
            .fg(fg);
        para.render(area, buf);
    }
}

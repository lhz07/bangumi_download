use crate::tui::{app::App, progress_bar::SpeedSum, qrcode_widget};
use std::io;

use crate::tui::progress_bar::Inc;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style, Styled},
    text::Line,
    widgets::{Block, Borders, Gauge, Scrollbar, ScrollbarOrientation, Tabs},
};

#[derive(Clone, Copy, PartialEq)]
pub enum CurrentScreen {
    Main,
    Downloading,
    Finished,
    Log,
}

pub enum Popup {
    DownloadFolder,
    Login,
    AddRSSLink,
}

pub enum InputState {
    NotInput,
    Text(String),
    SelectedAll(String),
}

impl InputState {
    pub fn to_selected(&mut self) {
        let old = std::mem::replace(self, Self::NotInput);
        if let Self::Text(str) = old {
            *self = Self::SelectedAll(str);
        } else {
            *self = old;
        }
    }
}

pub fn render(app: &mut App) -> io::Result<()> {
    app.terminal.draw(|f| {
        let downloading_tasks = app.downloading_state.progresses.len();
        let downloading_tab = if downloading_tasks > 0 {
            format!("Downloading ({})", downloading_tasks)
        } else {
            "Downloading".to_string()
        };
        // render tabs
        let tabs = Tabs::new([
            "Main".to_string(),
            downloading_tab,
            "Finished".to_string(),
            "Log".to_string(),
        ])
        .select(app.current_screen as usize);
        let main_layout =
            Layout::vertical([Constraint::Min(2), Constraint::Percentage(100)]).split(f.area());
        f.render_widget(tabs, main_layout[0]);

        if app.current_screen != CurrentScreen::Downloading {
            app.downloading_state
                .progresses
                .retain(|p| p.size() != p.current_size());
        }

        match app.current_screen {
            CurrentScreen::Main => {
                if let Some(popup) = &app.current_popup {
                    let vertical_layout = Layout::vertical([
                        Constraint::Percentage(10),
                        Constraint::Percentage(70),
                        Constraint::Percentage(20),
                    ])
                    .split(main_layout[1]);
                    let horizontal_layout = Layout::horizontal([
                        Constraint::Percentage(20),
                        Constraint::Percentage(60),
                        Constraint::Percentage(20),
                    ])
                    .split(vertical_layout[1]);
                    let popup_area = horizontal_layout[1];
                    match popup {
                        Popup::DownloadFolder => {
                            let popup_block = Block::default()
                                .title("Download a folder")
                                .title_alignment(ratatui::layout::Alignment::Center)
                                .borders(Borders::ALL);
                            f.render_widget(popup_block, popup_area);
                            let popup_layout = Layout::vertical([
                                Constraint::Percentage(10),
                                Constraint::Percentage(10),
                                Constraint::Percentage(70),
                                Constraint::Percentage(10),
                            ])
                            .margin(1)
                            .split(popup_area);
                            f.render_widget("Please enter the cid of the folder", popup_layout[0]);
                            if let InputState::Text(str) = &app.input_state {
                                f.render_widget(str, popup_layout[1]);
                            } else if let InputState::SelectedAll(str) = &app.input_state {
                                let str = str.clone().set_style(Style::default().bg(Color::Black));
                                f.render_widget(str, popup_layout[1]);
                            }
                        }
                        Popup::AddRSSLink => {}
                        Popup::Login => {
                            let vertical_layout = Layout::vertical([
                                Constraint::Fill(1),
                                Constraint::Min(22),
                                Constraint::Fill(1),
                            ])
                            .split(main_layout[1]);
                            let horizontal_layout = Layout::horizontal([
                                Constraint::Fill(1),
                                Constraint::Min(41),
                                Constraint::Fill(1),
                            ])
                            .split(vertical_layout[1]);
                            let popup_area = horizontal_layout[1];
                            let qrcode = qrcode_widget::QrcodeWidget::new();
                            f.render_widget(qrcode, popup_area);
                        }
                    }
                }
            }
            CurrentScreen::Downloading => {
                let state = &mut app.downloading_state;
                let horizontal_layout = Layout::horizontal([
                    Constraint::Length(main_layout[1].width - 3),
                    Constraint::Length(3),
                ])
                .split(main_layout[1]);
                let vertical_layout =
                    Layout::vertical([Constraint::Length(2), Constraint::Fill(1)])
                        .split(horizontal_layout[0]);
                let download_status_area = vertical_layout[0];
                let speed_sum = (state.progresses.speed() as f64 / 1000000.0 + 0.5) as u64;
                let line = Line::raw(format!(
                    "Downloading task(s): {}       Speed: {} MB/s",
                    state.progresses.len(),
                    speed_sum
                ));
                f.render_widget(line, download_status_area);
                let progresses_area = vertical_layout[1];
                let scroll_bar_area = horizontal_layout[1];
                let height = main_layout[1].height as usize;
                // 每个进度条占用 3 行：上下边框 + 内容
                let per_item_height = 3;
                // 可视的进度条个数
                let visible_count = height / per_item_height;
                // 计算可见区间
                state.offset = state
                    .offset
                    .min(state.progresses.len().saturating_sub(visible_count));
                let end = (state.offset + visible_count).min(state.progresses.len());
                state.scroll_state = state
                    .scroll_state
                    .content_length(state.progresses.len().saturating_sub(visible_count));
                // 为每个进度条生成一个长度为 per_item_height 的约束
                let constraints =
                    vec![Constraint::Length(per_item_height as u16); end - state.offset];
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints(constraints)
                    .split(progresses_area);
                let mut chunks_iter = chunks.iter();
                let mut j = 0;
                state.progresses.retain_mut(|p| {
                    let percent = p.pos();
                    let speed = (p.calculate_speed() as f64 / 1000000.0 + 0.5) as u64;
                    let current_size = (p.current_size() as f64 / 1000000.0 + 0.5) as u64;
                    let size = (p.size() as f64 / 1000000.0 + 0.5) as u64;
                    if j >= state.offset
                        && j < end
                        && let Some(chunk) = chunks_iter.next()
                    {
                        let gauge = Gauge::default()
                            .block(Block::default().borders(Borders::ALL).title(format!(
                                "{} {} MB / {} MB   {} MB/s",
                                p.name(),
                                current_size,
                                size,
                                speed
                            )))
                            .gauge_style(
                                Style::default()
                                    .fg(Color::Rgb(0, 212, 241))
                                    .bg(Color::Rgb(37, 50, 56)),
                            )
                            .percent(percent);
                        f.render_widget(gauge, *chunk);
                    }
                    j += 1;
                    percent != 100
                });
                f.render_stateful_widget(
                    Scrollbar::new(ScrollbarOrientation::VerticalRight)
                        .begin_symbol(Some("↑"))
                        .end_symbol(Some("↓")),
                    scroll_bar_area,
                    &mut state.scroll_state,
                );
            }
            CurrentScreen::Finished => {}
            CurrentScreen::Log => {
                let logs = tui_logger::TuiLoggerWidget::default()
                    .block(Block::default().title("Logs").borders(Borders::ALL))
                    .state(&app.log_widget_state);
                f.render_widget(logs, main_layout[1]);
                // f.render_stateful_widget(logs, main_layout[1], &mut app.log_widget_state);
            }
        }
    })?;
    Ok(())
}

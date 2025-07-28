use std::io;

use crate::{
    END_NOTIFY,
    socket_utils::{DownloadState, ReadSocketMsg, SocketMsg, SocketPath, WriteSocketMsg},
    tui::{events::LEvent, progress_bar::ProgressBar},
};
use futures::future::join;
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self, Event, KeyCode},
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Gauge, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use tokio::{
    net::unix::{OwnedReadHalf, OwnedWriteHalf},
    select,
    sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
    task::JoinHandle,
};

pub enum CurrentScreen {
    Main,
    Downloading,
    Finished,
    Log,
}
#[derive(Clone)]
pub struct ListState {
    offset: usize,
    scroll_state: ScrollbarState,
    progresses: Vec<ProgressBar>,
}

pub struct Handles {
    pub socket_handle: JoinHandle<Result<(), io::Error>>,
    pub ui_events_handle: JoinHandle<Result<(), io::Error>>,
}

pub struct App {
    current_screen: CurrentScreen,
    terminal: DefaultTerminal,
    downloading_state: ListState,
    finished_state: ListState,
    socket_tx: UnboundedSender<SocketMsg>,
}

impl App {
    pub fn initialize(
        terminal: DefaultTerminal,
        socket_path: SocketPath,
    ) -> (Self, UnboundedReceiver<LEvent>, Handles) {
        let downloading_state = ListState {
            offset: 0,
            scroll_state: ScrollbarState::new(0),
            progresses: Vec::new(),
        };
        let finished_state = downloading_state.clone();
        let (event_tx, event_rx) = unbounded_channel::<LEvent>();
        let (socket_tx, socket_rx) = unbounded_channel::<SocketMsg>();
        let socket_handle = tokio::spawn(Self::initialize_socket(
            event_tx.clone(),
            socket_rx,
            socket_path,
        ));
        let ui_events_handle = tokio::spawn(Self::receive_ui_events(event_tx));
        let handles = Handles {
            socket_handle,
            ui_events_handle,
        };
        let app = App {
            current_screen: CurrentScreen::Main,
            terminal,
            downloading_state,
            finished_state,
            socket_tx,
        };
        let _ = app.socket_tx.send(SocketMsg::SyncQuery);
        (app, event_rx, handles)
    }
    pub async fn receive_ui_events(tx: UnboundedSender<LEvent>) -> io::Result<()> {
        let handle = tokio::task::spawn_blocking(move || -> io::Result<()> {
            loop {
                let event = LEvent::Tui(event::read()?);
                if let Err(_) = tx.send(event) {
                    // log error
                }
            }
        });
        select! {
            result = handle => {result.unwrap()?}
            _ = END_NOTIFY.notified() => {}
        }
        Ok(())
    }
    pub async fn initialize_socket(
        tx: UnboundedSender<LEvent>,
        rx: UnboundedReceiver<SocketMsg>,
        socket_path: SocketPath,
    ) -> io::Result<()> {
        let stream = socket_path.to_stream().await?;
        let (read, write) = stream.split();
        let (read_result, write_result) = join(
            Self::read_socket(tx.clone(), read),
            Self::write_socket(rx, write),
        )
        .await;
        read_result?;
        write_result?;
        Ok(())
    }
    pub async fn read_socket(
        tx: UnboundedSender<LEvent>,
        mut read: OwnedReadHalf,
    ) -> io::Result<()> {
        loop {
            select! {
                result = read.read_msg() => {
                    if let Err(_) = tx.send(LEvent::Socket(result?)) {
                        // log error
                    }
                }
                _ = END_NOTIFY.notified() => {break;}
            }
        }
        Ok(())
    }
    pub async fn write_socket(
        mut rx: UnboundedReceiver<SocketMsg>,
        mut write: OwnedWriteHalf,
    ) -> io::Result<()> {
        while let Some(msg) = rx.recv().await {
            write.write_msg(msg).await?;
        }
        Ok(())
    }
    pub fn render(&mut self) -> io::Result<()> {
        self.terminal.draw(|f| {
            match &self.current_screen {
                CurrentScreen::Main => {}
                CurrentScreen::Downloading => {
                    let state = &mut self.downloading_state;
                    let area = f.area();
                    let main_layout = Layout::horizontal([
                        Constraint::Length(area.width - 3),
                        Constraint::Length(3),
                    ])
                    .split(area);
                    let height = area.height as usize;
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
                        .split(main_layout[0]);
                    let mut chunks1 = chunks.iter().enumerate();
                    let mut j = 0;
                    state.progresses.retain_mut(|p| {
                        let percent = p.pos();
                        let speed = (p.calculate_speed() as f64 / 1000000.0 + 0.5) as u64;
                        if j >= state.offset
                            && j < end
                            && let Some((i, chunk)) = chunks1.next()
                        {
                            let global_index = state.offset + i;
                            let gauge = Gauge::default()
                                .block(Block::default().borders(Borders::ALL).title(format!(
                                    "Task {} [诸神字幕组] 中二病也要谈恋爱！恋  {} MB/s",
                                    global_index, speed
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
                        main_layout[1],
                        &mut state.scroll_state,
                    );
                }
                CurrentScreen::Finished => {}
                CurrentScreen::Log => {}
            }
        })?;
        Ok(())
    }
    pub async fn event_loop(&mut self, mut rx: UnboundedReceiver<LEvent>) -> io::Result<()> {
        while let Some(event) = rx.recv().await {
            match event {
                LEvent::Tui(ui_event) => {
                    // deal with keyboard...
                    match ui_event {
                        Event::Key(key) => {
                            match key.code {
                                KeyCode::Char('q') => break, // q 退出
                                KeyCode::Down => {
                                    // 滚动下一条
                                    let scroll_down = |state: &mut ListState| {
                                        if state.offset + 1 < state.progresses.len() {
                                            state.offset += 1;
                                            state.scroll_state =
                                                state.scroll_state.position(state.offset);
                                        }
                                    };
                                    match &mut self.current_screen {
                                        CurrentScreen::Downloading => {
                                            let state = &mut self.downloading_state;
                                            scroll_down(state);
                                        }
                                        CurrentScreen::Finished => {
                                            let state = &mut self.finished_state;
                                            scroll_down(state);
                                        }
                                        _ => {}
                                    }
                                }
                                KeyCode::Up => {
                                    // 滚动上一条
                                    let scroll_up = |state: &mut ListState| {
                                        if state.offset > 0 {
                                            state.offset -= 1;
                                            state.scroll_state =
                                                state.scroll_state.position(state.offset);
                                        }
                                    };
                                    match &mut self.current_screen {
                                        CurrentScreen::Downloading => {
                                            let state = &mut self.downloading_state;
                                            scroll_up(state);
                                        }
                                        CurrentScreen::Finished => {
                                            let state = &mut self.finished_state;
                                            scroll_up(state);
                                        }
                                        _ => {}
                                    }
                                }
                                _ => {}
                            }
                        }
                        _ => (),
                    }
                }
                LEvent::Render => {
                    // render ui here
                    self.render()?;
                }
                LEvent::Socket(msg) => {
                    // deal with socket message here
                    match msg {
                        SocketMsg::Download(msg) => match msg.state {
                            DownloadState::Start((name, size)) => {
                                let bar = ProgressBar::new(name, msg.id, size);
                                self.downloading_state.progresses.push(bar);
                            }
                            DownloadState::Downloading(delta) => {
                                for progress in &mut self.downloading_state.progresses {
                                    if progress.id() == msg.id {
                                        progress.inc(delta);
                                    }
                                }
                            }
                            DownloadState::Finished => {
                                for progress in &mut self.downloading_state.progresses {
                                    if progress.id() == msg.id {
                                        progress.inc_to_finished();
                                    }
                                }
                            }
                        },
                        _ => (),
                    }
                }
            }
        }
        Ok(())
    }
}

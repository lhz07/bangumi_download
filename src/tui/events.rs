use std::io;

use crate::{
    END_NOTIFY,
    config_manager::SafeSend,
    socket_utils::{DownloadState, SocketMsg},
    tui::{
        app::{App, ListState},
        progress_bar::{Inc, ProgressBar},
        ui::{self, CurrentScreen, InputState, Popup},
    },
};
use ratatui::crossterm::event::Event;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc::UnboundedReceiver;

pub enum LEvent {
    Tui(Event),
    Render,
    Socket(SocketMsg),
}

impl LEvent {
    pub async fn event_loop(app: &mut App, mut rx: UnboundedReceiver<LEvent>) -> io::Result<()> {
        while let Some(event) = rx.recv().await {
            match event {
                LEvent::Tui(ui_event) => {
                    // deal with keyboard...
                    match ui_event {
                        Event::Key(key) => {
                            if Self::handle_key_events(app, key) {
                                break;
                            }
                        }
                        _ => (),
                    }
                }
                LEvent::Render => {
                    // render ui here
                    ui::render(app)?;
                }
                LEvent::Socket(msg) => {
                    // TODO: change to a more efficient way
                    // deal with socket message here
                    match msg {
                        SocketMsg::Download(msg) => match msg.state {
                            DownloadState::Start((name, size)) => {
                                log::trace!(
                                    "received a socket download start msg, name: {}, id: {}, size: {}",
                                    name,
                                    msg.id,
                                    size
                                );
                                let bar = ProgressBar::new(name, msg.id, size);
                                app.downloading_state.progresses.push(bar);
                            }
                            DownloadState::Downloading(delta) => {
                                app.count += 1;
                                // log::trace!("current msg count: {}", app.count);
                                for progress in &mut app.downloading_state.progresses {
                                    if progress.id() == msg.id {
                                        progress.inc(delta);
                                        break;
                                    }
                                }
                            }
                            DownloadState::Finished => {
                                log::trace!(
                                    "received a socket download finish msg, id: {}",
                                    msg.id
                                );
                                for progress in &mut app.downloading_state.progresses {
                                    if progress.id() == msg.id {
                                        progress.inc_to_finished();
                                        break;
                                    }
                                }
                            }
                        },
                        SocketMsg::SyncResp(info) => {
                            let progresses = info
                                .progresses
                                .into_iter()
                                .map(|bar| bar.to_progress_bar())
                                .collect::<Vec<_>>();
                            app.downloading_state.progresses = progresses;
                        }
                        SocketMsg::Ok(info) => {
                            log::info!("{}", info);
                        }
                        SocketMsg::Error(error) => {
                            log::error!("{}", error);
                        }
                        // no need to handle these messages
                        SocketMsg::DownloadFolder(_) => (),
                        SocketMsg::Null => (),
                        SocketMsg::SyncQuery => (),
                    }
                }
            }
        }
        Ok(())
    }

    /// bool: whether to exit
    pub fn handle_key_events(app: &mut App, key: KeyEvent) -> bool {
        match key.modifiers {
            KeyModifiers::CONTROL => {
                if let KeyCode::Char('a') = key.code {
                    if let InputState::Text(_) = &mut app.input_state {
                        app.input_state.to_selected();
                    }
                }
                return false;
            }
            _ => (),
        }
        match key.code {
            KeyCode::Char(char) => {
                match &mut app.input_state {
                    InputState::NotInput => match char {
                        // press 'q' to exit
                        'q' => {
                            if let None = app.current_popup {
                                END_NOTIFY.notify_waiters();
                                return true;
                            }
                        }
                        // press '1' to switch to main screen
                        '1' => {
                            app.current_screen = CurrentScreen::Main;
                        }
                        '2' => {
                            app.current_screen = CurrentScreen::Downloading;
                        }
                        '3' => {
                            app.current_screen = CurrentScreen::Finished;
                        }
                        '4' => {
                            app.current_screen = CurrentScreen::Log;
                        }
                        char if app.current_screen == CurrentScreen::Main => {
                            match char {
                                // download a folder
                                'd' => {
                                    app.input_state = InputState::Text(String::new());
                                    app.current_popup = Some(Popup::DownloadFolder);
                                }
                                // login to cloud
                                'l' => {
                                    app.current_popup = Some(Popup::Login);
                                }
                                _ => (),
                            }
                        }
                        _ => (),
                    },
                    InputState::Text(str) => {
                        str.push(char);
                    }
                    InputState::SelectedAll(_) => {
                        app.input_state = InputState::Text(char.to_string());
                    }
                }
            }
            KeyCode::Esc => {
                // assume the popup takes the keyboard focus and we don't want to restore it
                if app.current_popup.is_some() {
                    app.current_popup = None;
                    app.input_state = InputState::NotInput;
                } else if app.current_screen != CurrentScreen::Main {
                    app.current_screen = CurrentScreen::Main;
                }
            }
            KeyCode::Backspace => {
                if let InputState::Text(str) = &mut app.input_state {
                    str.pop();
                } else if let InputState::SelectedAll(_) = &mut app.input_state {
                    app.input_state = InputState::Text(String::new());
                }
            }
            KeyCode::Enter => {
                if let Some(popup) = &app.current_popup {
                    match popup {
                        Popup::DownloadFolder => {
                            if let InputState::Text(str) = &mut app.input_state
                                && !str.is_empty()
                            {
                                let str = std::mem::replace(str, String::new());
                                let msg = SocketMsg::DownloadFolder(str);
                                app.socket_tx.send_msg(msg);
                            }
                        }
                        Popup::AddRSSLink => {}
                        _ => (),
                    }
                    app.input_state = InputState::NotInput;
                    app.current_popup = None;
                }
            }
            KeyCode::Down => {
                let scroll_down = |state: &mut ListState| {
                    if state.offset + 1 < state.progresses.len() {
                        state.offset += 1;
                        state.scroll_state = state.scroll_state.position(state.offset);
                    }
                };
                match &mut app.current_screen {
                    CurrentScreen::Downloading => {
                        let state = &mut app.downloading_state;
                        scroll_down(state);
                    }
                    CurrentScreen::Finished => {
                        let state = &mut app.finished_state;
                        scroll_down(state);
                    }
                    CurrentScreen::Log => {
                        app.log_widget_state
                            .transition(tui_logger::TuiWidgetEvent::NextPageKey);
                    }
                    _ => {}
                }
            }
            KeyCode::Up => {
                let scroll_up = |state: &mut ListState| {
                    if state.offset > 0 {
                        state.offset -= 1;
                        state.scroll_state = state.scroll_state.position(state.offset);
                    }
                };
                match &mut app.current_screen {
                    CurrentScreen::Downloading => {
                        let state = &mut app.downloading_state;
                        scroll_up(state);
                    }
                    CurrentScreen::Finished => {
                        let state = &mut app.finished_state;
                        scroll_up(state);
                    }
                    CurrentScreen::Log => {
                        app.log_widget_state
                            .transition(tui_logger::TuiWidgetEvent::PrevPageKey);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        false
    }
}

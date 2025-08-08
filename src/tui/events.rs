use std::io;

use crate::{
    END_NOTIFY,
    config_manager::SafeSend,
    socket_utils::{ClientMsg, DownloadState, ServerMsg},
    tui::{
        app::{App, ListState},
        notification_widget::Notification,
        progress_bar::{BasicBar, SimpleBar},
        ui::{self, CurrentScreen, InputState, Popup},
    },
};
use ratatui::crossterm::event::Event;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tokio::sync::mpsc::UnboundedReceiver;

pub enum LEvent {
    Tui(Event),
    Render,
    Socket(ServerMsg),
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
                    // deal with socket message here
                    match msg {
                        ServerMsg::Download(msg) => match msg.state {
                            DownloadState::Start(ptr) => {
                                let (name, size) = *ptr;
                                log::trace!(
                                    "received a socket download start msg, name: {}, {}, size: {}",
                                    name,
                                    msg.id,
                                    size
                                );
                                let bar = SimpleBar::new(name.into_string(), size);
                                app.downloading_state.progress_suit.add(msg.id, bar);
                            }
                            DownloadState::Downloading(_) => (),
                            DownloadState::Finished => {
                                log::trace!("received a socket download finish msg, {}", msg.id);
                                if let Some(bar) =
                                    app.downloading_state.progress_suit.get_bar_mut(msg.id)
                                {
                                    bar.inc_to_finished();
                                } else {
                                    log::error!(
                                        "received a finished msg, but can not find its progress bar"
                                    );
                                    app.socket_tx.send_msg(ClientMsg::SyncQuery);
                                }
                            }
                        },
                        ServerMsg::DownloadSync(state) => {
                            // log::trace!("received a socket download sync msg");
                            let mut is_lossy = false;
                            for s in &state {
                                if let Some(bar) =
                                    app.downloading_state.progress_suit.get_bar_mut(s.id)
                                {
                                    bar.set_current_size(s.current_size);
                                    bar.set_current_speed(s.current_speed);
                                } else {
                                    log::error!(
                                        "received a sync msg, but can not find its progress bar"
                                    );
                                    is_lossy = true;
                                }
                            }
                            // maybe we should not be too strict
                            // if is_lossy || state.len() != app.downloading_state.progress_suit.len()
                            // {
                            if is_lossy {
                                log::error!("send SyncQuery because of DownloadSync");
                                app.socket_tx.send_msg(ClientMsg::SyncQuery);
                            }
                        }
                        ServerMsg::SyncResp(info) => {
                            app.downloading_state.progress_suit = info.progresses;
                        }
                        ServerMsg::Ok(info) => {
                            log::info!("{}", info);
                            let noti = Notification::new("Success".to_string(), info.into_string());
                            app.notifications_queue.push_back(noti);
                        }
                        ServerMsg::Error(ptr) => {
                            let (info, error) = *ptr;
                            log::error!("{}", error);
                            let noti = Notification::new("Failed".to_string(), info.into_string());
                            app.notifications_queue.push_back(noti);
                        }
                        ServerMsg::Info(info) => {}
                        ServerMsg::LoginState(state) => {}
                        ServerMsg::LoginUrl(url) => {}
                        // no need to handle these messages
                        ServerMsg::Null => (),
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
                                let msg = ClientMsg::DownloadFolder(str.into_boxed_str());
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
                    if state.offset + 1 < state.progress_suit.len() {
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
            KeyCode::Right => {
                if let InputState::SelectedAll(_) = &mut app.input_state {
                    app.input_state.to_unselected();
                }
            }
            _ => {}
        }
        false
    }
}

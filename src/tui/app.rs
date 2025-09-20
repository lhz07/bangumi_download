use crate::config_manager::SafeSend;
use crate::socket_utils::{
    Anime, AsyncReadSocketMsg, AsyncWriteSocketMsg, ClientMsg, Filter, SocketPath,
};
use crate::tui::events::LEvent;
use crate::tui::loading_widget::LoadingState;
use crate::tui::notification_widget::Notification;
use crate::tui::progress_bar::{ProgressSuit, SimpleBar};
use crate::tui::ui::{CurrentScreen, InputState, Popup};
use crate::{END_NOTIFY, READY_TO_EXIT};
use futures::future::join;
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self};
use ratatui::widgets::{ListState as TuiListState, ScrollbarState};
use std::collections::VecDeque;
use std::io;
use std::time::Duration;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::select;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;
use tui_logger::TuiWidgetState;

impl SafeSend<ClientMsg> for UnboundedSender<ClientMsg> {
    fn send_msg(&self, msg: ClientMsg) {
        if let Err(e) = self.send(msg) {
            log::error!("It seems that the Receiver of ClientMsg is closed too early, error: {e}");
        }
    }
}

impl SafeSend<LEvent> for UnboundedSender<LEvent> {
    fn send_msg(&self, msg: LEvent) {
        if let Err(e) = self.send(msg) {
            log::error!("It seems that the Receiver of LEvent is closed too early, error: {e}");
        }
    }
}

pub struct ListState {
    pub(crate) offset: usize,
    pub(crate) scroll_state: ScrollbarState,
    pub(crate) progress_suit: ProgressSuit<SimpleBar>,
}

impl ListState {
    pub fn new() -> Self {
        ListState {
            offset: 0,
            scroll_state: ScrollbarState::new(0),
            progress_suit: ProgressSuit::new(),
        }
    }
}

impl Default for ListState {
    fn default() -> Self {
        Self::new()
    }
}

pub struct Handles {
    pub socket_handle: JoinHandle<Result<(), io::Error>>,
    pub ui_events_handle: JoinHandle<Result<(), io::Error>>,
}

pub struct App {
    pub(crate) current_screen: CurrentScreen,
    pub(crate) current_popup: Option<Popup>,
    pub(crate) qrcode_url: Result<Box<str>, &'static str>,
    pub(crate) input_state: InputState,
    pub(crate) terminal: DefaultTerminal,
    pub(crate) downloading_state: ListState,
    pub(crate) finished_state: ListState,
    pub(crate) log_widget_state: TuiWidgetState,
    pub(crate) socket_tx: UnboundedSender<ClientMsg>,
    pub(crate) notifications_queue: VecDeque<Notification>,
    pub(crate) rss_data: Vec<Anime>,
    pub(crate) rss_state: TuiListState,
    pub(crate) filter_id_state: TuiListState,
    pub(crate) filter_rule_state: TuiListState,
    pub(crate) loading_state: Option<LoadingState>,
    pub(crate) is_logged_in: bool,
    pub(crate) filters: Vec<Filter>,
}

impl App {
    pub fn initialize(
        terminal: DefaultTerminal,
        socket_path: SocketPath,
    ) -> (Self, UnboundedReceiver<LEvent>, Handles) {
        // Set max_log_level to Trace
        tui_logger::init_logger(log::LevelFilter::Trace).unwrap();

        // Set default level for unknown targets to Trace
        tui_logger::set_default_level(log::LevelFilter::Trace);
        let downloading_state = ListState::new();
        let finished_state = ListState::new();
        // the event loop chanel
        let (event_tx, event_rx) = unbounded_channel::<LEvent>();
        // the socket channel, write the msg to socket
        let (socket_tx, socket_rx) = unbounded_channel::<ClientMsg>();
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
        let log_widget_state = TuiWidgetState::new();
        let app = App {
            current_screen: CurrentScreen::Main,
            current_popup: None,
            qrcode_url: Err("Loading..."),
            input_state: InputState::NotInput,
            terminal,
            downloading_state,
            finished_state,
            socket_tx,
            log_widget_state,
            notifications_queue: VecDeque::new(),
            rss_data: Vec::new(),
            rss_state: TuiListState::default(),
            filter_id_state: TuiListState::default(),
            filter_rule_state: TuiListState::default(),
            loading_state: None,
            is_logged_in: false,
            filters: Vec::new(),
        };
        app.socket_tx.send_msg(ClientMsg::SyncQuery);
        (app, event_rx, handles)
    }
    pub async fn receive_ui_events(tx: UnboundedSender<LEvent>) -> io::Result<()> {
        let wait_poll = async || event::poll(Duration::from_millis(50));
        loop {
            select! {
                poll = wait_poll() => {
                    if poll? {
                        let event = LEvent::Tui(event::read()?);
                        tx.send_msg(event);
                    }
                    let event = LEvent::Render;
                    if let Err(e) = tx.send(event) {
                        log::error!("It seems that the Receiver of LEvent is closed too early, error: {e}");
                        break;
                    }
                }
                _ = END_NOTIFY.notified() => {
                    break;
                }
            }
        }
        Ok(())
    }
    pub async fn initialize_socket(
        tx: UnboundedSender<LEvent>,
        rx: UnboundedReceiver<ClientMsg>,
        socket_path: SocketPath,
    ) -> io::Result<()> {
        let stream = socket_path.to_stream().await?;
        let (read, write) = stream.into_split();
        let (read_result, write_result) = join(
            Self::read_socket(tx.clone(), read),
            Self::write_socket(rx, write),
        )
        .await;
        let read_result = read_result.inspect_err(|e| log::error!("socket read error: {e}"));
        write_result.inspect_err(|e| log::error!("socket write error: {e}"))?;
        read_result?;
        Ok(())
    }
    pub async fn read_socket(
        tx: UnboundedSender<LEvent>,
        mut read: OwnedReadHalf,
    ) -> io::Result<()> {
        loop {
            select! {
                result = read.read_msg() => {
                    match result {
                        Ok(msg) => tx.send_msg(LEvent::Socket(msg)),
                        Err(e) => {
                            if READY_TO_EXIT.load(std::sync::atomic::Ordering::Relaxed) {
                                break;
                            } else {
                                Err(e)?;
                            }
                        }
                    }
                }
                _ = END_NOTIFY.notified() => {break;}
            }
        }
        Ok(())
    }
    pub async fn write_socket(
        mut rx: UnboundedReceiver<ClientMsg>,
        mut write: OwnedWriteHalf,
    ) -> io::Result<()> {
        loop {
            select! {
                recv_result = rx.recv() => {
                    if let Some(msg) = recv_result {
                        log::trace!("write msg: {:?}", msg);
                        write.write_msg(msg).await?;
                    }
                }
                _ = END_NOTIFY.notified() => {break;}
            }
        }

        Ok(())
    }
}

use std::{collections::VecDeque, io, time::Duration};

use crate::{
    END_NOTIFY,
    config_manager::SafeSend,
    socket_utils::{ClientMsg, ReadSocketMsg, SocketPath, WriteSocketMsg},
    tui::{
        events::LEvent,
        notification_widget::Notification,
        progress_bar::{ProgressSuit, SimpleBar},
        ui::{CurrentScreen, InputState, Popup},
    },
};
use futures::future::join;
use ratatui::{
    DefaultTerminal,
    crossterm::event::{self},
    widgets::ScrollbarState,
};
use tokio::{
    net::unix::{OwnedReadHalf, OwnedWriteHalf},
    select,
    sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel},
    task::JoinHandle,
};
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

pub struct Handles {
    pub socket_handle: JoinHandle<Result<(), io::Error>>,
    pub ui_events_handle: JoinHandle<Result<(), io::Error>>,
}

pub struct App {
    pub(crate) current_screen: CurrentScreen,
    pub(crate) current_popup: Option<Popup>,
    pub(crate) input_state: InputState,
    pub(crate) terminal: DefaultTerminal,
    pub(crate) downloading_state: ListState,
    pub(crate) finished_state: ListState,
    pub(crate) log_widget_state: TuiWidgetState,
    pub(crate) socket_tx: UnboundedSender<ClientMsg>,
    pub(crate) notifications_queue: VecDeque<Notification>,
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
        for i in 1..50 {
            log::info!("{i}");
        }
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
            input_state: InputState::NotInput,
            terminal,
            downloading_state,
            finished_state,
            socket_tx,
            log_widget_state,
            notifications_queue: VecDeque::new(),
        };
        app.socket_tx.send_msg(ClientMsg::SyncQuery);
        (app, event_rx, handles)
    }
    pub async fn receive_ui_events(tx: UnboundedSender<LEvent>) -> io::Result<()> {
        loop {
            if event::poll(Duration::from_millis(50))? {
                let event = LEvent::Tui(event::read()?);
                tx.send_msg(event);
            }
            let event = LEvent::Render;
            if let Err(e) = tx.send(event) {
                log::error!("It seems that the Receiver of LEvent is closed too early, error: {e}");
                break;
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
                    tx.send_msg(LEvent::Socket(result?));
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

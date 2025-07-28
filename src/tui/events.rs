use crate::socket_utils::SocketMsg;
use ratatui::crossterm::event::Event;

pub enum LEvent {
    Tui(Event),
    Render,
    Socket(SocketMsg),
}

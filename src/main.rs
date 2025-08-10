use std::{io, mem::ManuallyDrop, process::ExitCode};

use bangumi_download::{
    BROADCAST_RX, BROADCAST_TX, END_NOTIFY, ERROR_STATUS, TX,
    config_manager::SafeSend,
    id::Id,
    main_proc::initialize,
    socket_utils::{ClientMsg, ServerMsg, SocketPath, SocketState, SocketStateDetect},
    tui::{app::App, events::LEvent},
};
use futures::future::join3;
use tokio::sync::mpsc::unbounded_channel;

// we need to give the macro a var or let it use the global var
// macro_rules! printf {
//     () => {
//         PRINT.print(format!("\n"))
//     };
//     ($($arg:tt)*) => {{
//         #[allow(static_mut_refs)]
//         unsafe{PRINT.print(format!($($arg)*));}
//     }};
// }

#[tokio::main]
async fn main() -> ExitCode {
    let socket_path = SocketPath::new("bangumi_download.socket");
    if let SocketState::Working = socket_path.try_connect() {
        let terminal = ratatui::init();
        let (mut app, rx, handles) = App::initialize(terminal, socket_path);
        let results = join3(
            LEvent::event_loop(&mut app, rx),
            handles.socket_handle,
            handles.ui_events_handle,
        )
        .await;
        ratatui::restore();
        let check_results = || -> Result<(), io::Error> {
            results
                .0
                .inspect_err(|e| eprintln!("event loop error: {e}"))?;
            results
                .1
                .unwrap()
                .inspect_err(|e| eprintln!("socket error: {e}"))?;
            results
                .2
                .unwrap()
                .inspect_err(|e| eprintln!("ui events handle error: {e}"))?;
            Ok(())
        };
        if check_results().is_err() {
            return ExitCode::FAILURE;
        }
        return ExitCode::SUCCESS;
    } else {
        let _ = BROADCAST_TX.clone();
        let rx = unsafe {
            let dr = Box::from_raw(BROADCAST_RX);
            let rx = ManuallyDrop::into_inner(*dr);
            BROADCAST_RX = std::ptr::null_mut();
            rx
        };

        let mut exit_now = false;
        ctrlc::set_handler(move || {
            if exit_now {
                println!("force quit!");
                std::process::exit(1);
            } else {
                println!("\nExiting...");
                BROADCAST_TX.send_msg(ServerMsg::Exit);
                exit_now = true;
                // wait for handling the exit message
                std::thread::sleep(std::time::Duration::from_millis(50));
                END_NOTIFY.notify_waiters();
                // The 2 lines below will end the process!
                println!("try to drop TX");
                drop(TX.swap(None));
                println!("dropped TX, waiting for config_manager to finish...");
            }
        })
        .unwrap();

        let mut listener = match socket_path.initial_listener() {
            Ok(listener) => listener,
            Err(e) => {
                eprintln!("Can not bind unix socket, Error: {e}");
                return ExitCode::FAILURE;
            }
        };

        let config_manager = initialize().await;
        let (stream_read_tx, stream_read_rx) = unbounded_channel::<(Id, ClientMsg)>();
        listener.listening(rx, stream_read_tx, stream_read_rx).await;
        drop(listener);
        config_manager.await.unwrap();
    }
    if ERROR_STATUS.load(std::sync::atomic::Ordering::Relaxed) {
        return ExitCode::FAILURE;
    } else {
        return ExitCode::SUCCESS;
    }
}

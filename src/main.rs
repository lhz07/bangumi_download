use std::{io, mem::ManuallyDrop, process::ExitCode};

use bangumi_download::{
    BROADCAST_RX, BROADCAST_TX, END_NOTIFY, ERROR_STATUS, EXIT_NOW, TX,
    id::Id,
    main_proc::initialize,
    socket_utils::{ClientMsg, SocketPath, SocketState, SocketStateDetect},
    tui::{app::App, events::LEvent},
};
use futures::future::join3;
use tokio::{signal, sync::mpsc::unbounded_channel};

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
            results.0?;
            results.1.unwrap()?;
            results.2.unwrap()?;
            Ok(())
        };
        if let Err(e) = check_results() {
            log::error!("Error: {e}");
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
        let (stream_read_tx, stream_read_rx) = unbounded_channel::<(Id, ClientMsg)>();
        let config_manager = initialize().await;
        let mut listener = match socket_path.initial_listener() {
            Ok(listener) => listener,
            Err(e) => {
                eprintln!("Can not bind unix socket, Error: {e}");
                return ExitCode::FAILURE;
            }
        };
        ctrlc::set_handler(|| {
            if EXIT_NOW.load(std::sync::atomic::Ordering::Relaxed) {
                println!("force quit!");
                std::process::exit(1);
            }
        })
        .unwrap();
        tokio::select! {
            _ = signal::ctrl_c() => {
                println!("\nExiting...");
                EXIT_NOW.store(true, std::sync::atomic::Ordering::Relaxed);
                drop(listener);
                END_NOTIFY.notify_waiters();
                // The 2 lines below will end the process!
                println!("try to drop TX");
                drop(TX.swap(None));
                println!("dropped TX, waiting for config_manager to finish...");
                config_manager.await.unwrap();
            },
            _ = listener.listening(rx, stream_read_tx, stream_read_rx) => {}
        }
    }
    if ERROR_STATUS.load(std::sync::atomic::Ordering::Relaxed) {
        return ExitCode::FAILURE;
    } else {
        return ExitCode::SUCCESS;
    }
}

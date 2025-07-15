use std::process::ExitCode;

use bangumi_download::{
    END_NOTIFY, ERROR_STATUS, EXIT_NOW, TX,
    cli_tools::Cli,
    main_proc::initialize,
    socket_utils::{SocketPath, SocketState, SocketStateDetect},
};
use tokio::signal;

#[tokio::main]
async fn main() -> ExitCode {
    let socket_path = SocketPath::new("bangumi_download.socket");
    if let SocketState::Working = socket_path.try_connect() {
        if let Err(e) = Cli::cli_main(socket_path).await {
            eprintln!("Socket error: {e}");
            return ExitCode::FAILURE;
        }
    } else {
        let config_manager = initialize().await;
        let listener = match socket_path.initial_listener() {
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
            _ = listener.listening() => {}
        }
    }
    if ERROR_STATUS.load(std::sync::atomic::Ordering::Relaxed) {
        return ExitCode::FAILURE;
    } else {
        return ExitCode::SUCCESS;
    }
}

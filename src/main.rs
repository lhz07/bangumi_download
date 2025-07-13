use std::process::ExitCode;

use bangumi_download::{
    END_NOTIFY, ERROR_STATUS, EXIT_NOW, TX,
    cli_tools::{Args, Cli, Command},
    main_proc::initial,
    socket_utils::{SocketPath, SocketState, SocketStateDetect},
};
use clap::Parser;
use tokio::signal;

#[tokio::main]
async fn main() -> ExitCode {
    let socket_path = SocketPath::new("bangumi_download.socket");
    if let SocketState::Working = socket_path.try_connect() {
        let arg = Args::parse();
        let mut stream = socket_path.to_stream().await.unwrap();
        match arg.command {
            Some(cmd) => {
                stream.write_str("short").await.unwrap();
                match cmd {
                    Command::Update => Cli::update(&mut stream).await,
                    Command::AddLink { link } => Cli::add_a_link(&mut stream, Some(&link)).await,
                    Command::DelLink => Cli::del_a_link(&mut stream).await,
                }
            }
            None => {
                stream.write_str("keep-alive").await.unwrap();
                loop {
                    println!(
                        "\n请输入想要执行的操作: \n1.添加RSS链接\n2.删除RSS链接\n3.添加字幕组过滤器\n4.删除字幕组过滤器\n5.添加单个磁链下载\n6.下载文件夹\n7.退出程序\n"
                    );
                    let mut input = String::new();
                    std::io::stdin().read_line(&mut input).unwrap();
                    let select = input.trim();
                    match select {
                        "1" => Cli::add_a_link(&mut stream, None).await,
                        "2" => Cli::del_a_link(&mut stream).await,
                        "3" => Cli::add_subgroup_filter(&mut stream).await,
                        "4" => Cli::del_subgroup_filter(&mut stream).await,
                        "5" => Cli::add_single_magnet_download(&mut stream).await,
                        "6" => Cli::download_a_folder(&mut stream).await,
                        "7" => {
                            println!("正在退出...");
                            break;
                        }
                        _ => continue,
                    }
                }
            }
        }
    } else {
        let config_manager = initial().await;
        let listener = socket_path.to_listener().unwrap();
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

use bangumi_download::{
    CLIENT_WITH_RETRY, REFRESH_DOWNLOAD, REFRESH_DOWNLOAD_SLOW, TX,
    alist_manager::{check_cookies, check_is_alist_working, get_alist_token},
    cli_tools::{Args, Cli, Command},
    config_manager::{CONFIG, ConfigManager, Message, modify_config},
    main_proc::{initial, refresh_download, refresh_download_slow, refresh_rss},
    socket_utils::{SocketListener, SocketPath, SocketState, SocketStateDetect},
    update_rss::rss_receive,
};
use clap::Parser;
use tokio::signal;

#[tokio::main]
async fn main() {
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
                        "\n请输入想要执行的操作: \n1.添加RSS链接\n2.删除RSS链接\n3.添加字幕组过滤器\n4.删除字幕组过滤器\n5.添加单个磁链下载\n6.退出程序\n"
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
                        "6" => {
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
        tokio::select! {
            _ = signal::ctrl_c() => {
                println!("\nExiting...");
                drop(listener);
                // The 2 lines below will end the process!
                drop(TX.write().await.take());
                config_manager.await.unwrap();
            },
            _ = listener.listening() => {}
        }
    }

    // if let SocketState::Working = socket_path.try_connect() {
    //     manager.write("hello").await.unwrap();
    //     loop {
    //     println!(
    //         "\n请输入想要执行的操作: \n1.添加RSS链接\n2.删除RSS链接\n3.添加字幕组过滤器\n4.删除字幕组过滤器\n5.添加单个磁链下载\n6.退出程序\n"
    //     );
    //     let mut input = String::new();
    //     std::io::stdin().read_line(&mut input).unwrap();
    //     let select = input.trim();
    //     match select {
    //         "1" => {
    //             println!("请输入要添加的RSS链接:");
    //             let mut input = String::new();
    //             std::io::stdin().read_line(&mut input).unwrap();
    //             let rss_link = input.trim();
    //             if rss_link.is_empty() {
    //                 println!("RSS链接不能为空");
    //                 continue;
    //             }
    //             let tx = TX.read().await.clone().unwrap();
    //             let old_config = CONFIG.read().await.get_value().clone();
    //             rss_receive(tx, rss_link, &old_config, CLIENT_WITH_RETRY.clone())
    //                 .await
    //                 .unwrap();
    //         }
    //         "6" => {
    //             println!("正在退出...");
    //             break;
    //         }
    //         _ => continue,
    //     }
    // }
    // } else {
    //     manager.bind().unwrap();
    //     let config_manager = initial().await;
    //     tokio::select! {
    //         _ = signal::ctrl_c() => {
    //             println!("Exiting...");
    //             drop(TX.write().await.take());
    //             config_manager.await.unwrap();
    //             drop(manager);
    //         }
    //         _ = manager.listening() => {}
    //     }
    // }
}

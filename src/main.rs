use bangumi_download::{
    CLIENT_WITH_RETRY, REFRESH_DOWNLOAD, TX,
    alist_manager::{
        check_cookies, get_alist_token,
    },
    config_manager::{CONFIG, Config, Message, modify_config},
    main_proc::{refresh_download, refresh_rss},
    update_rss::rss_receive,
};

#[tokio::main]
async fn main() {
    // -------------------------------------------------------------------------
    // initial config
    if let Err(error) = Config::initial_config().await {
        eprintln!("can not initial config, error: {error}");
        std::process::exit(1);
    }
    // launch config write thread
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
    *(TX.write().await) = Some(tx);
    let config_manager = tokio::spawn(modify_config(rx));
    // -------------------------------------------------------------------------
    let username = CONFIG.read().await.get_value()["user"]["name"]
        .as_str()
        .unwrap()
        .to_string();
    let password = CONFIG.read().await.get_value()["user"]["password"]
        .as_str()
        .unwrap()
        .to_string();
    println!("{:?}", get_alist_token(&username, &password).await);
    println!("{:?}", check_cookies().await);
    let _rss_refresh_handle = tokio::spawn(refresh_rss());
    if CONFIG.read().await.get_value()["downloading_hash"]
        .as_array()
        .unwrap()
        .len()
        > 0
    {
        let download_handle = tokio::spawn(refresh_download());
        REFRESH_DOWNLOAD.lock().await.replace(download_handle);
    }
    loop {
        println!(
            "\n请输入想要执行的操作: \n1.添加RSS链接\n2.删除RSS链接\n3.添加字幕组过滤器\n4.删除字幕组过滤器\n5.添加单个磁链下载\n6.退出程序\n"
        );
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).unwrap();
        let select = input.trim();
        match select {
            "1" => {
                println!("请输入要添加的RSS链接:");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input).unwrap();
                let rss_link = input.trim();
                if rss_link.is_empty() {
                    println!("RSS链接不能为空");
                    continue;
                }
                let tx = TX.read().await.clone().unwrap();
                let old_config = CONFIG.read().await.get_value().clone();
                rss_receive(tx, rss_link, &old_config, CLIENT_WITH_RETRY.clone())
                    .await
                    .unwrap();
            }
            "6" => {
                println!("正在退出...");
                break;
            }
            _ => continue,
        }
    }
    drop(TX.write().await.take());
    config_manager.await.unwrap();
}

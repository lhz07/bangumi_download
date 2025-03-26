use bangumi_download::{config_manager::initial_config, config_manager::CONFIG, login_with_qrcode, update_rss};

#[tokio::main]
async fn main() {
    // println!("{:?}", login_with_qrcode::login_with_qrcode("wechatmini").await);
    if let Err(error) = initial_config() {
        eprintln!("can not initial config, error: {error}");
        std::process::exit(1);
    }
    // println!("{:?}", CONFIG.read().await);
    let urls = vec![
        "https://mikanime.tv/RSS/Bangumi?bangumiId=3523&subgroupid=611",
        "https://mikanime.tv/RSS/Bangumi?bangumiId=3519&subgroupid=370",
        "https://mikanime.tv/RSS/Bangumi?bangumiId=3546&subgroupid=370",
        "https://mikanime.tv/RSS/Bangumi?bangumiId=3524&subgroupid=583",
        "https://mikanime.tv/RSS/Bangumi?bangumiId=3535&subgroupid=370",
    ];
    update_rss::start_rss_receive(urls).await;
}

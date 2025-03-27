use bangumi_download::{alist_manager::get_alist_token, config_manager::{initial_config, CONFIG}};

// #[tokio::main]
fn main() {
    // println!("{:?}", login_with_qrcode::login_with_qrcode("wechatmini").await);
    if let Err(error) = initial_config() {
        eprintln!("can not initial config, error: {error}");
        std::process::exit(1);
    }
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let config = CONFIG.read().await.get_value()["user"].clone();
        let _ = get_alist_token(config["name"].as_str().unwrap(), config["password"].as_str().unwrap()).await;
    })
    // println!("{:?}", CONFIG.read().await);
    // let urls = vec![
    //     "https://mikanime.tv/RSS/Bangumi?bangumiId=3523&subgroupid=611",
    //     "https://mikanime.tv/RSS/Bangumi?bangumiId=3519&subgroupid=370",
    //     "https://mikanime.tv/RSS/Bangumi?bangumiId=3546&subgroupid=370",
    //     "https://mikanime.tv/RSS/Bangumi?bangumiId=3524&subgroupid=583",
    //     "https://mikanime.tv/RSS/Bangumi?bangumiId=3535&subgroupid=370",
    // ];
    // update_rss::start_rss_receive(urls).await;
}

use bangumi_download::{
    TX,
    alist_manager::{
        check_cookies, get_alist_token, get_file_list, get_file_raw_url, update_alist_cookies,
    },
    cloud_manager::{cloud_download, del_cloud_task, download_file, get_tasks_list},
    config_manager::{CONFIG, Config, Message, MessageCmd, MessageType, modify_config},
    update_rss::{get_a_magnet_link, get_all_episode_magnet_links, start_rss_receive},
};
use futures::FutureExt;
use serde_json::Value;

#[tokio::main]
async fn main() {
    // println!("{:?}", login_with_qrcode::login_with_qrcode("wechatmini").await);

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


    let username = CONFIG.read().await.get_value()["user"]["name"].as_str().unwrap().to_string();
    let password = CONFIG.read().await.get_value()["user"]["password"].as_str().unwrap().to_string();
    println!("{:?}", get_alist_token(&username, &password).await);
    println!("{:?}", check_cookies().await);
    // match get_tasks_list().await{
    //     Ok(r) => {
    //         for i in r{
    //             println!("{:?}", i["name"]);
    //             println!("{:?}", i["info_hash"]);
    //         }
    //     },
    //     Err(e) => eprintln!("{:?}", e),
    // }
    // println!("{:?}", get_file_list("/115/云下载").await);
    // println!("{:?}", del_cloud_task("e5f48854a62160fa29509c759e71b13dfd7f416b").await);
    let (name, url) = get_file_raw_url("/115/云下载/[LoliHouse] Kono Kaisha ni Suki na Hito ga Imasu - 06 [WebRip 1080p HEVC-10bit AAC SRTx2].mkv/[LoliHouse] Kono Kaisha ni Suki na Hito ga Imasu - 06 [WebRip 1080p HEVC-10bit AAC SRTx2].mkv").await.unwrap();
    println!("{:?}", download_file(&url, &name).await);
    // tokio::runtime::Runtime::new().unwrap().block_on(async {
    // let config = CONFIG.read().await.get_value()["user"].clone();
    // println!("{:?}", get_alist_token(config["name"].as_str().unwrap(), config["password"].as_str().unwrap()).await);
    // let download_urls = vec!["magnet:?xt=urn:btih:9a2070854c2cb47dd743d57d1cc417544b1facef&tr=http%3a%2f%2ft.nyaatracker.com%2fannounce&tr=http%3a%2f%2ftracker.kamigami.org%3a2710%2fannounce&tr=http%3a%2f%2fshare.camoe.cn%3a8080%2fannounce&tr=http%3a%2f%2fopentracker.acgnx.se%2fannounce&tr=http%3a%2f%2fanidex.moe%3a6969%2fannounce&tr=http%3a%2f%2ft.acg.rip%3a6699%2fannounce&tr=https%3a%2f%2ftr.bangumi.moe%3a9696%2fannounce&tr=udp%3a%2f%2ftr.bangumi.moe%3a6969%2fannounce&tr=http%3a%2f%2fopen.acgtracker.com%3a1096%2fannounce&tr=udp%3a%2f%2ftracker.opentrackr.org%3a1337%2fannounce".to_string()];
    // println!("{:?}", cloud_download(&download_urls).await);
    // println!("{:?}", get_a_magnet_link("https://mikanime.tv/Home/Episode/9d22370519e85dde9c9521a289812d30b7b0321b").await);
    // let username = CONFIG.read().await.get_value()["user"]["name"].as_str().unwrap().to_string();
    // let password = CONFIG.read().await.get_value()["user"]["password"].as_str().unwrap().to_string();
    // println!("{:?}", get_alist_token(&username, &password).await);
    // // println!("{:?}", update_alist_cookies().await);
    // println!("{:?}", get_file_raw_url("/115/云下载/[LoliHouse] Kono Kaisha ni Suki na Hito ga Imasu - 06 [WebRip 1080p HEVC-10bit AAC SRTx2].mkv/[LoliHouse] Kono Kaisha ni Suki na Hito ga Imasu - 06 [WebRip 1080p HEVC-10bit AAC SRTx2].mkv").await);
    // let urls = vec![
    //     "https://mikanime.tv/RSS/Bangumi?bangumiId=3523&subgroupid=611",
    //     "https://mikanime.tv/RSS/Bangumi?bangumiId=3519&subgroupid=370",
    //     "https://mikanime.tv/RSS/Bangumi?bangumiId=3546&subgroupid=370",
    //     "https://mikanime.tv/RSS/Bangumi?bangumiId=3524&subgroupid=583",
    //     "https://mikanime.tv/RSS/Bangumi?bangumiId=3535&subgroupid=370",
    // ];
    // start_rss_receive(urls).await;
    // download_all_episode("3523", "611").await;
    // });
    // println!("{:?}", test_download());
    // println!("{:?}", CONFIG.read().await);
    loop {
        let mut input = String::new();
        std::io::stdin().read_line(&mut input).unwrap();
        let select = input.trim();
        match select {
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

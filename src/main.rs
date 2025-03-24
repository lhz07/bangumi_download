use bangumi_download::rss_receive;
use futures::future;

// async fn download_one_by_one(urls: &Vec<&str>){
//     for i in urls{
//         rss_receive(i).await;
//     }
// }

#[tokio::main]
async fn main() {
    let urls = vec![
        "https://mikanime.tv/RSS/Bangumi?bangumiId=3523&subgroupid=611",
        // "https://mikanime.tv/RSS/Bangumi?bangumiId=3519&subgroupid=370",
        // "https://mikanime.tv/RSS/Bangumi?bangumiId=3546&subgroupid=370",
        // "https://mikanime.tv/RSS/Bangumi?bangumiId=3524&subgroupid=583",
        // "https://mikanime.tv/RSS/Bangumi?bangumiId=3535&subgroupid=370",
    ];
    let rss_receive_fut: Vec<_> = urls.iter().map(|url|rss_receive(url)).collect();
    future::join_all(rss_receive_fut).await;
    // download_one_by_one(&urls).await;
}

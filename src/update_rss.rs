use crate::config_manager::{modify_config, Message, CONFIG};
use futures::future;
use scraper::{Html, Selector};
use serde_json::Value;
use std::{error::Error, pin::Pin, vec};
use tokio::{fs, sync::mpsc};

async fn get_response_text(url: &str, client: reqwest::Client) -> Result<String, reqwest::Error> {
    Ok(client.get(url).send().await?.text().await?)
}

pub async fn start_rss_receive(urls: Vec<&str>) {
    // get the message channel
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
    // clone the tx
    let mut txs = Vec::new();
    for _ in 1..urls.len() {
        txs.push(tx.clone());
    }
    txs.push(tx);
    // create client
    let client = reqwest::Client::new();
    // read the config file

    // let file_content = fs::read_to_string("config.json")
    //     .await
    //     .expect("can not read config.json");
    let old_config: Value = CONFIG.read().await.get_value().clone();
    // create the sender futures
    let mut futs: Vec<Pin<Box<dyn Future<Output = Result<(), Box<dyn Error>>>>>> = Vec::new();
    for (index, tx) in txs.into_iter().enumerate() {
        futs.push(Box::pin(rss_receive(tx, urls[index], &old_config, client.clone())));
    }
    // create the receiver future
    futs.push(Box::pin(modify_config(rx)));
    // get the results
    let results = future::join_all(futs).await;

    for result in results {
        if let Err(error) = result {
            eprintln!("{}", error);
        }
    }
}

async fn get_subgroup_name(url: &str, client: reqwest::Client) -> Option<String> {
    let response = match get_response_text(url, client).await {
        Ok(response) => response,
        Err(error) => {
            eprintln!("can not open {url}, error: {error}");
            return None;
        }
    };
    let resource = Html::parse_document(&response);
    let selector = Selector::parse("a.magnet-link-wrap").unwrap();
    let sub_name = resource.select(&selector).next()?.text().next()?;
    Some(sub_name.to_string())
}

pub async fn rss_receive(
    tx: mpsc::UnboundedSender<Message>,
    url: &str,
    old_config: &Value,
    client: reqwest::Client,
) -> Result<(), Box<dyn Error>> {
    let response = client.get(url).send().await?.text().await?;
    let rss_content = feedparser::from_str(&response).ok_or("can not parse rss!")?;
    // println!("{:?}", rss_content);
    // println!("{:?}", rss_content["rss"]["channel"]["item"].get(0));
    let items = rss_content.items().ok_or("can not found items!")?;
    // let rss_content = RssChannel::from_str(&response)?;
    let latest_item = items.first().ok_or("can not found latest item!")?;
    let mut split_ani_sub = rss_content["link"]
        .as_str()
        .unwrap_or_default()
        .split("bangumiId=")
        .nth(1)
        .unwrap_or_default()
        .split("&subgroupid=");
    let ani_id = split_ani_sub
        .next()
        .ok_or("can not found ani_id!")?
        .to_string();
    let sub_id = split_ani_sub
        .next()
        .ok_or("can not found sub_id!")?
        .to_string();
    let sub_name = get_subgroup_name(latest_item.link().ok_or("can not found link!")?, client)
        .await
        .unwrap_or_default();
    let bangumi_name = rss_content["title"]
        .as_str()
        .unwrap_or_default()
        .split(" - ")
        .nth(1)
        .unwrap_or(rss_content["title"].as_str().unwrap_or_default())
        .to_string();
    let title = format!("[{sub_name}] {bangumi_name}");
    let latest_update = latest_item.torrent().ok_or("can not found pubDate!")?["pubDate"]
        .as_str()
        .ok_or("can not found pubDate!")?
        .to_string();
    let bangumi_id = format!("{ani_id}&{sub_id}");
    // check if the bangumi updates and is it first time to be added
    let old_bangumi_dict = old_config["bangumi"].as_object().ok_or("can not find old bangumi!")?;
    if !old_bangumi_dict.contains_key(&bangumi_id){
        // TODO: download_all_episode(ani_id, sub_id, title)
        // write to config
        let msg = Message::new(
            vec!["bangumi".to_string(), format!("{ani_id}&{sub_id}")],
            latest_update,
            false,
        );
        tx.send(msg)?;
        let msg = Message::new(vec!["rss_links".to_string(), title], url.to_owned(), false);
        tx.send(msg)?;
    }else if latest_update == old_bangumi_dict[&bangumi_id]{
        println!("{title} 无更新");
    }else {
        let mut item_iter = rss_content["item"].as_array().ok_or("can not find item!")?.iter().rev();
        for item in &mut item_iter {
            let pub_date = item["torrent"]["pubDate"].as_str().ok_or("can not found pub_date!")?;
            if pub_date == old_bangumi_dict[&bangumi_id]{
                break;
            }
        }
        println!("获取到以下剧集：");
        for item in item_iter{
            println!("{}", item["title"].as_str().ok_or("title not found!")?);
        }
    }

    
    // println!("{} {} {} {:?}", ani_id, sub_id, title, last_update);
    // for i in rss_content.into_items() {
    //     println!("{}", &i.title().expect("MUST HAVE TITLE!"));
    // }
    Ok(())
}

use crate::{
    CLIENT, CLIENT_WITH_RETRY, ERROR_STATUS, TX,
    cloud_manager::cloud_download,
    config_manager::{CONFIG, Message, MessageCmd, MessageType, modify_config},
};
use futures::future::{self, join_all};
use reqwest::Client;
use reqwest_middleware::ClientWithMiddleware;
use scraper::{Element, Html, Selector};
use serde_json::{Value, json};
use std::{cell::Cell, error::Error, pin::Pin, time::Duration, vec};
use tokio::sync::mpsc;
use tokio_retry::{
    Retry,
    strategy::{self, ExponentialBackoff, FibonacciBackoff},
};

async fn get_response_text(
    url: &str,
    client: ClientWithMiddleware,
) -> Result<String, Box<dyn Error>> {
    let response = client.get(url).send().await?;
    if response.status().is_success() {
        Ok(response.text().await?)
    } else {
        Err(format!("Request failed with status: {}", response.status()).into())
    }
}

pub async fn start_rss_receive(urls: Vec<&str>) {
    // create client
    // read the config
    let old_config: Value = CONFIG.read().await.get_value().clone();
    // create the sender futures
    // let mut futs: Vec<Pin<Box<dyn Future<Output = Result<(), Box<dyn Error>>>>>> = Vec::new();
    let mut futs = Vec::new();
    for url in urls {
        let tx = match TX.read().await.clone() {
            Some(tx) => tx,
            None => return,
        };
        futs.push(rss_receive(tx, url, &old_config, CLIENT_WITH_RETRY.clone()));
    }
    // create the receiver future
    // futs.push(Box::pin(modify_config(rx)));
    // get the results
    let results = future::join_all(futs).await;

    for result in results {
        if let Err(error) = result {
            eprintln!("{}", error);
        }
    }
}

pub fn filter_episode<'a>(items: &'a Vec<Value>, filter: &Value, sub_id: &str) -> Vec<&'a str> {
    let default_filter = filter["default"].as_array().unwrap();
    let mut item_links: Vec<&str> = Vec::new();
    if filter.as_object().unwrap().contains_key(sub_id) {
        let sub_filter = filter[&sub_id].as_array().unwrap();
        item_links = items
            .iter()
            .filter(|item| {
                sub_filter.iter().any(|key_filter| {
                    item["title"]
                        .as_str()
                        .unwrap()
                        .contains(key_filter.as_str().unwrap())
                })
            })
            .map(|item| {
                println!("{}", item["title"].as_str().unwrap());
                item["link"].as_str().unwrap()
            })
            .collect();
    }
    for i in items {
        if i["title"].as_str().unwrap().contains("内封") {
            item_links.push(i["link"].as_str().unwrap());
            println!("{}", i["title"].as_str().unwrap());
        }
    }
    if item_links.is_empty() {
        for i in items {
            for j in default_filter {
                if i["title"].as_str().unwrap().contains(j.as_str().unwrap()) {
                    item_links.push(i["link"].as_str().unwrap());
                    println!("{}", i["title"].as_str().unwrap());
                    break;
                }
            }
        }
    }
    if item_links.is_empty() {
        for i in items {
            item_links.push(i["link"].as_str().unwrap());
            println!("{}", i["title"].as_str().unwrap());
        }
    }
    item_links
}

pub async fn get_all_episode_magnet_links(
    ani_id: &str,
    sub_id: &str,
    filter: &Value,
) -> Option<Vec<String>> {
    let url = format!(
        "https://mikanime.tv/Home/ExpandEpisodeTable?bangumiId={ani_id}&subtitleGroupId={sub_id}&take=100"
    );
    let client = CLIENT_WITH_RETRY.clone();
    let response = match get_response_text(&url, client).await {
        Ok(text) => text,
        Err(error) => {
            eprintln!("Error: {error}");
            return None;
        }
    };
    let soup = Html::parse_document(&response);
    let selector = Selector::parse("a.magnet-link-wrap").unwrap();
    let elements = soup.select(&selector);
    // for i in elements{
    //     println!("{}", i.text().collect::<String>());
    //     println!("{}", i.next_sibling_element()?.value().attr("data-clipboard-text")?);
    // }
    let mut items = Vec::<Value>::new();
    // let items = elements.map(|element|{
    //     let title = element.text().collect::<String>();
    //     let link = element.next_sibling_element()?.value().attr("data-clipboard-text")?;
    //     json!({"title": title, "link": link});
    // });
    for element in elements {
        items.push(json!({"title": element.text().collect::<String>(), 
                        "link": element.next_sibling_element()?.value().attr("data-clipboard-text")?}));
    }
    let magnet_links = filter_episode(&items, filter, sub_id)
        .iter()
        .map(|link| link.to_string())
        .collect();
    // println!("{:?}", magnet_links);
    Some(magnet_links)
}

async fn get_subgroup_name(url: &str, client: ClientWithMiddleware) -> Option<String> {
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

pub async fn get_all_magnet(item_urls: Vec<&str>) -> Result<Vec<String>, &str> {
    let futs = item_urls
        .iter()
        .map(|url| get_a_magnet_link(url))
        .collect::<Vec<_>>();
    let results = join_all(futs).await;
    println!("process links");
    let mut magnet_links = Vec::new();
    for i in results {
        match i {
            Some(link) => magnet_links.push(link),
            None => {
                println!("return error");
                return Err("Can not get the magnet link!");
            }
        }
    }
    // println!("return the links");
    Ok(magnet_links)
    // println!("{:?}", results);
}

pub async fn get_a_magnet_link(url: &str) -> Option<String> {
    let try_times = Cell::new(0);
    let response = match Retry::spawn(FibonacciBackoff::from_millis(5000).take(3), async || {
        if try_times.get() > 0 {
            eprintln!("can not open {url}, waiting for retry.");
        }
        try_times.set(1);
        let client = CLIENT_WITH_RETRY.clone();
        let res = get_response_text(url, client).await?;
        Ok::<String, Box<dyn Error>>(res)
    })
    .await
    {
        Ok(response) => response,
        Err(error) => {
            eprintln!("can not open {url}, error: {error}, already tried 5 times");
            return None;
        }
    };
    println!("got the response!");
    let resource = Html::parse_document(&response);
    let selector = Selector::parse("a[href]").unwrap();
    let magnet_link = resource
        .select(&selector)
        .filter_map(|element| element.value().attr("href"))
        .find(|href| href.starts_with("magnet:"))
        .map(|href| href.to_string())?;
    Some(magnet_link)
}

pub async fn rss_receive(
    tx: mpsc::UnboundedSender<Message>,
    url: &str,
    old_config: &Value,
    client: ClientWithMiddleware,
) -> Result<(), Box<dyn Error>> {
    let response = client.get(url).send().await?.text().await?;
    let rss_content = feedparser::from_str(&response).ok_or("can not parse rss!")?;
    // println!("{:?}", rss_content);
    // println!("{:?}", rss_content["rss"]["channel"]["item"].get(0));
    let items = rss_content.items().ok_or("can not found items!")?;
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
    let old_bangumi_dict = old_config["bangumi"].as_object().unwrap();
    let mut magnet_links: Vec<String> = Vec::new();
    if !old_bangumi_dict.contains_key(&bangumi_id) {
        // TODO: download_all_episode(ani_id, sub_id, title)
        // write to config
        let msg = Message::new(
            vec!["bangumi".to_string(), format!("{ani_id}&{sub_id}")],
            MessageType::Text(latest_update),
            MessageCmd::Replace,
        );
        tx.send(msg)?;
        match get_all_episode_magnet_links(&ani_id, &sub_id, &old_config["filter"]).await {
            Some(links) => magnet_links = links,
            None => (),
        }
        let msg = Message::new(
            vec!["rss_links".to_string(), title.to_string()],
            MessageType::Text(url.to_owned()),
            MessageCmd::Replace,
        );
        tx.send(msg)?;
    } else if latest_update == old_bangumi_dict[&bangumi_id] {
        println!("{title} 无更新");
    } else {
        // println!("waiting...");
        // tokio::time::sleep(Duration::from_secs(5)).await;
        let mut item_iter = rss_content["item"]
            .as_array()
            .ok_or("can not find item in rss!")?
            .iter()
            .rev();
        for item in &mut item_iter {
            let pub_date = item["torrent"]["pubDate"]
                .as_str()
                .ok_or("can not found pub_date!")?;
            if pub_date == old_bangumi_dict[&bangumi_id] {
                break;
            }
        }
        let new_items = item_iter.map(|item| item.clone()).collect::<Vec<_>>();
        println!("获取到以下剧集：");
        let filter = &old_config["filter"];
        let item_links = filter_episode(&new_items, filter, &sub_id);
        magnet_links = match get_all_magnet(item_links).await {
            Ok(links) => links,
            Err(error) => {
                eprintln!("Error: {:?}", error);
                *ERROR_STATUS.write().await = true;
                drop(TX.write().await.take());
                return Err("Can not get magnet links!".into());
            }
        };
        let msg = Message::new(
            vec!["bangumi".to_string(), format!("{ani_id}&{sub_id}")],
            MessageType::Text(latest_update),
            MessageCmd::Replace,
        );
        tx.send(msg)?;
    }
    if !magnet_links.is_empty() {
        // println!("waiting...");
        // tokio::time::sleep(Duration::from_secs(6)).await;
        match cloud_download(&magnet_links).await {
            Ok(hash_list) => {
                let mut hash_ani = serde_json::Map::new();
                for i in &hash_list {
                    hash_ani.insert(i.clone(), Value::String(title.clone()));
                }
                let msg = Message::new(
                    vec!["hash_ani".to_string()],
                    MessageType::Map(hash_ani),
                    MessageCmd::Append,
                );
                tx.send(msg).unwrap();
                let msg = Message::new(
                    vec!["downloading_hash".to_string()],
                    MessageType::List(hash_list),
                    MessageCmd::Append,
                );
                tx.send(msg).expect("can not send to config thread!");
            }
            Err(error) => {
                eprintln!("Error: {:?}", error);
                let msg = Message::new(
                    vec!["magnets".to_string(), title.to_string()],
                    MessageType::List(magnet_links),
                    MessageCmd::Append,
                );
                tx.send(msg).expect("can not send to config thread!");
                *ERROR_STATUS.write().await = true;
                drop(TX.write().await.take());
                return Err("Can not add magnet to cloud!".into());
            }
        }
    }
    // println!("{} {} {} {:?}", ani_id, sub_id, title, last_update);
    // for i in rss_content.into_items() {
    //     println!("{}", &i.title().expect("MUST HAVE TITLE!"));
    // }
    Ok(())
}

use crate::{
    CLIENT_WITH_RETRY, ERROR_STATUS, TX,
    cloud_manager::cloud_download,
    config_manager::{CONFIG, Message, MessageCmd, MessageType},
    main_proc::restart_refresh_download,
};
use futures::future::{self, join_all};
use reqwest_middleware::ClientWithMiddleware;
use scraper::{Element, Html, Selector};
use serde_json::{Value, json};
use std::{
    sync::{Arc, atomic::AtomicI32},
    vec,
};
use tokio::sync::{Notify, mpsc};
use tokio_retry::{Retry, strategy::FibonacciBackoff};

async fn get_response_text(
    url: &str,
    client: ClientWithMiddleware,
) -> Result<String, anyhow::Error> {
    let response = client.get(url).send().await?;
    if response.status().is_success() {
        Ok(response.text().await?)
    } else {
        Err(anyhow::format_err!(
            "Request failed with status: {}",
            response.status()
        ))
    }
}

pub async fn start_rss_receive(urls: Vec<&str>) {
    // read the config
    let old_config: Value = CONFIG.read().await.get_value().clone();
    // create the sender futures
    let mut futs = Vec::new();
    for url in urls {
        let tx = match TX.read().await.clone() {
            Some(tx) => tx,
            None => return,
        };
        futs.push(rss_receive(tx, url, &old_config, CLIENT_WITH_RETRY.clone()));
    }
    // get the results
    let results = future::join_all(futs).await;
    for result in results {
        if let Err(error) = result {
            eprintln!("{}", error);
        }
    }
}

pub fn filter_episode<'a>(items: &'a Vec<Value>, filter: &Value, sub_id: &str) -> Vec<&'a str> {
    let default_filters = filter["default"].as_array().unwrap();
    let mut best_filter: Option<&str> = None;
    let empty_filter: Vec<Value> = Vec::new();
    let candidate_filters = match filter[&sub_id].as_array() {
        Some(sub_filters) => sub_filters.iter().chain(default_filters.iter()),
        None => empty_filter.iter().chain(default_filters),
    };
    'outer: for candidate_filter in candidate_filters {
        for item in items {
            if item["title"]
                .as_str()
                .unwrap()
                .contains(candidate_filter.as_str().unwrap())
            {
                best_filter = Some(candidate_filter.as_str().unwrap());
                break 'outer;
            }
        }
    }
    match best_filter {
        Some(best_filter) => items
            .iter()
            .filter_map(|item| {
                if item["title"].as_str().unwrap().contains(best_filter) {
                    println!("{}", item["title"].as_str().unwrap());
                    Some(item["link"].as_str().unwrap())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>(),
        None => items
            .iter()
            .map(|item| {
                println!("{}", item["title"].as_str().unwrap());
                item["link"].as_str().unwrap()
            })
            .collect::<Vec<_>>(),
    }
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
    let mut items = Vec::<Value>::new();
    for element in elements {
        items.push(json!({"title": element.text().collect::<String>(), 
                        "link": element.next_sibling_element()?.value().attr("data-clipboard-text")?}));
    }
    let magnet_links = filter_episode(&items, filter, sub_id)
        .iter()
        .map(|link| link.to_string())
        .collect();
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
    Ok(magnet_links)
}

pub async fn get_a_magnet_link(url: &str) -> Option<String> {
    let try_times = AtomicI32::new(0);
    let response = match Retry::spawn(FibonacciBackoff::from_millis(5000).take(3), async || {
        if try_times.load(std::sync::atomic::Ordering::SeqCst) > 0 {
            eprintln!("can not open {url}, waiting for retry.");
        }
        try_times.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let client = CLIENT_WITH_RETRY.clone();
        let res = get_response_text(url, client).await?;
        Ok::<String, anyhow::Error>(res)
    })
    .await
    {
        Ok(response) => response,
        Err(error) => {
            eprintln!("can not open {url}, error: {error}, already tried 3 times");
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
) -> Result<(), anyhow::Error> {
    let response = client.get(url).send().await?.text().await?;
    let rss_content =
        feedparser::from_str(&response).ok_or_else(|| anyhow::Error::msg("can not parse rss!"))?;
    let items = rss_content
        .items()
        .ok_or_else(|| anyhow::Error::msg("can not found items!"))?;
    let latest_item = items
        .first()
        .ok_or_else(|| anyhow::Error::msg("can not found latest item!"))?;
    let mut split_ani_sub = rss_content["link"]
        .as_str()
        .unwrap_or_default()
        .split("bangumiId=")
        .nth(1)
        .unwrap_or_default()
        .split("&subgroupid=");
    let ani_id = split_ani_sub
        .next()
        .ok_or_else(|| anyhow::Error::msg("can not found ani_id!"))?
        .to_string();
    let sub_id = split_ani_sub
        .next()
        .ok_or_else(|| anyhow::Error::msg("can not found sub_id!"))?
        .to_string();
    let sub_name = get_subgroup_name(
        latest_item
            .link()
            .ok_or_else(|| anyhow::Error::msg("can not found link!"))?,
        client,
    )
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
    let latest_update = latest_item
        .torrent()
        .ok_or_else(|| anyhow::Error::msg("can not found pubDate!"))?["pubDate"]
        .as_str()
        .ok_or_else(|| anyhow::Error::msg("can not found pubDate!"))?
        .to_string();
    let bangumi_id = format!("{ani_id}&{sub_id}");
    // check if the bangumi updates and is it first time to be added
    let old_bangumi_dict = old_config["bangumi"].as_object().unwrap();
    let mut magnet_links: Vec<String> = Vec::new();
    if !old_bangumi_dict.contains_key(&bangumi_id) {
        // write to config
        let msg = Message::new(
            vec!["bangumi".to_string(), format!("{ani_id}&{sub_id}")],
            MessageType::Text(latest_update),
            MessageCmd::Replace,
            None,
        );
        tx.send(msg)?;
        if let Some(links) =
            get_all_episode_magnet_links(&ani_id, &sub_id, &old_config["filter"]).await
        {
            magnet_links = links
        }
        let msg = Message::new(
            vec!["rss_links".to_string(), title.to_string()],
            MessageType::Text(url.to_owned()),
            MessageCmd::Replace,
            None,
        );
        tx.send(msg)?;
    } else if latest_update == old_bangumi_dict[&bangumi_id] {
        println!("{title} 无更新");
    } else {
        let mut item_iter = rss_content["item"]
            .as_array()
            .ok_or_else(|| anyhow::Error::msg("can not find item in rss!"))?
            .iter()
            .rev();
        for item in &mut item_iter {
            let pub_date = item["torrent"]["pubDate"]
                .as_str()
                .ok_or_else(|| anyhow::Error::msg("can not found pub_date!"))?;
            if pub_date == old_bangumi_dict[&bangumi_id] {
                break;
            }
        }
        let new_items = item_iter.cloned().collect::<Vec<_>>();
        println!("获取到以下剧集：");
        let filter = &old_config["filter"];
        let item_links = filter_episode(&new_items, filter, &sub_id);
        magnet_links = match get_all_magnet(item_links).await {
            Ok(links) => links,
            Err(error) => {
                eprintln!("Error: {:?}", error);
                *ERROR_STATUS.write().await = true;
                drop(TX.write().await.take());
                return Err(anyhow::anyhow!("Can not get magnet links!"));
            }
        };
        let msg = Message::new(
            vec!["bangumi".to_string(), format!("{ani_id}&{sub_id}")],
            MessageType::Text(latest_update),
            MessageCmd::Replace,
            None,
        );
        tx.send(msg)?;
    }
    if let Some(magnets) = old_config["magnets"][&title].as_array() {
        magnet_links.append(
            &mut magnets
                .iter()
                .map(|i| i.as_str().unwrap().to_string())
                .collect::<Vec<_>>(),
        );
    }
    if !magnet_links.is_empty() {
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
                    None,
                );
                tx.send(msg).unwrap();
                let msg = Message::new(
                    vec!["magnets".to_string(), title.to_string()],
                    MessageType::None,
                    MessageCmd::DeleteKey,
                    None,
                );
                tx.send(msg).unwrap();
                let notify = Arc::new(Notify::new());
                let msg = Message::new(
                    vec!["downloading_hash".to_string()],
                    MessageType::List(hash_list),
                    MessageCmd::Append,
                    Some(notify.clone()),
                );
                tx.send(msg).expect("can not send to config thread!");
                notify.notified().await;
                restart_refresh_download().await;
            }
            Err(error) => {
                eprintln!("Error: {:?}", error);
                let msg = Message::new(
                    vec!["magnets".to_string(), title.to_string()],
                    MessageType::List(magnet_links),
                    MessageCmd::Append,
                    None,
                );
                tx.send(msg).expect("can not send to config thread!");
                *ERROR_STATUS.write().await = true;
                drop(TX.write().await.take());
                return Err(anyhow::anyhow!("Can not add magnet to cloud!"));
            }
        }
    }
    Ok(())
}

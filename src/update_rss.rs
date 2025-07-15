use crate::{
    CLIENT_WITH_RETRY, TX,
    cloud_manager::cloud_download,
    config_manager::{CONFIG, Config, Message, SafeSend},
    errors::{CatError, CloudError, DownloadError},
    main_proc::{restart_refresh_download, restart_refresh_download_slow},
};
use futures::future::{self, join_all};
use quick_xml::de;
use regex::Regex;
use reqwest_middleware::ClientWithMiddleware;
use scraper::{Element, Html, Selector};
use serde::Deserialize;
use std::{collections::HashMap, sync::Arc};
use tokio::sync::{Notify, mpsc};

#[derive(Debug, Deserialize)]
pub struct RSS {
    channel: Channel,
}

#[derive(Debug, Deserialize)]
pub struct Channel {
    title: String,
    link: String,
    item: Vec<Item>,
}

#[derive(Debug, Deserialize)]
pub struct Item {
    title: String,
    link: String,
    torrent: Torrent,
}

#[derive(Debug, Deserialize)]
pub struct Torrent {
    #[serde(rename = "pubDate")]
    pub_date: String,
}

pub struct LItem {
    title: String,
    link: String,
}

pub trait Filter {
    fn title(&self) -> &str;
    fn link(&self) -> &str;
}

impl Filter for Item {
    fn title(&self) -> &str {
        &self.title
    }
    fn link(&self) -> &str {
        &self.link
    }
}

impl Filter for LItem {
    fn title(&self) -> &str {
        &self.title
    }
    fn link(&self) -> &str {
        &self.link
    }
}

pub async fn get_response_text(
    url: &str,
    client: ClientWithMiddleware,
) -> Result<String, DownloadError> {
    let response = client.get(url).send().await?;
    if response.status().is_success() {
        Ok(response.text().await?)
    } else {
        Err(format!("Request failed with status: {}", response.status()))?
    }
}

pub async fn start_rss_receive(urls: Vec<&String>) -> Result<(), CatError> {
    // read the config
    let old_config = CONFIG.load_full();
    // create the sender futures
    let tx = TX.load_full().ok_or(CatError::Exit)?;
    let mut futs = Vec::new();
    for url in urls {
        futs.push(rss_receive(
            &tx,
            url,
            &old_config,
            CLIENT_WITH_RETRY.clone(),
        ));
    }
    // get the results
    println!("waiting for refreshing rss");
    let results = future::join_all(futs).await;
    println!("rss refresh finished");
    for result in results {
        match result {
            Ok(()) => (),
            Err(CatError::Cloud(CloudError::Download(DownloadError::Request(e)))) => {
                eprintln!("{}", e)
            }
            Err(e) => Err(e)?,
        }
    }
    println!("restart refresh download in rss receive");
    restart_refresh_download().await?;
    restart_refresh_download_slow().await?;
    println!("finish restart refresh download");
    Ok(())
}

pub fn filter_episode<'a, T: Filter>(
    items: &'a Vec<T>,
    filter: &HashMap<String, Vec<String>>,
    sub_id: &str,
) -> Vec<&'a str> {
    let default_filters = &filter["default"];
    let mut best_filter: Option<&str> = None;
    let empty_filter: Vec<String> = Vec::new();
    let candidate_filters = match filter.get(sub_id) {
        Some(sub_filters) => sub_filters.iter().chain(default_filters.iter()),
        None => empty_filter.iter().chain(default_filters.iter()),
    };
    'outer: for candidate_filter in candidate_filters {
        for item in items {
            if item.title().contains(candidate_filter) {
                best_filter = Some(candidate_filter);
                break 'outer;
            }
        }
    }
    match best_filter {
        Some(best_filter) => items
            .into_iter()
            .filter_map(|item| {
                if item.title().contains(best_filter) {
                    println!("{}", item.title());
                    Some(item.link())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>(),
        None => items
            .into_iter()
            .map(|item| {
                println!("{}", item.title());
                item.link()
            })
            .collect::<Vec<_>>(),
    }
}

pub async fn get_all_episode_magnet_links(
    ani_id: &str,
    sub_id: &str,
    filter: &HashMap<String, Vec<String>>,
) -> Result<Vec<String>, CatError> {
    let url = format!(
        "https://mikanime.tv/Home/ExpandEpisodeTable?bangumiId={ani_id}&subtitleGroupId={sub_id}&take=100"
    );
    let client = CLIENT_WITH_RETRY.clone();
    let response = get_response_text(&url, client)
        .await
        .map_err(|e| CatError::Parse(format!("Get all episode magnet links error: {e}")))?;
    let soup = Html::parse_document(&response);
    let selector =
        Selector::parse("a.magnet-link-wrap").expect("html element selector must be valid!");
    let elements = soup.select(&selector);
    let mut items = Vec::<LItem>::new();
    for element in elements {
        items.push(LItem {
            title: element.text().collect::<String>(),
            link: element
                .next_sibling_element()
                .and_then(|element| element.value().attr("data-clipboard-text"))
                .and_then(|s| Some(s.to_string()))
                .ok_or(CatError::Parse(
                    "parse all episode magnet links error".to_string(),
                ))?,
        });
    }
    let magnet_links = filter_episode(&items, filter, sub_id)
        .iter()
        .map(|link| link.to_string())
        .collect();
    Ok(magnet_links)
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
    let selector =
        Selector::parse("a.magnet-link-wrap").expect("html element selector must be valid!");
    let sub_name = resource.select(&selector).next()?.text().next()?;
    Some(sub_name.to_string())
}

pub async fn get_all_magnet(item_urls: Vec<&str>) -> Result<Vec<String>, CatError> {
    let client = CLIENT_WITH_RETRY.clone();
    let futs = item_urls
        .iter()
        .map(|url| get_a_magnet_link(url, client.clone()))
        .collect::<Vec<_>>();
    let results = join_all(futs).await;
    println!("process links");
    let magnet_links = results.into_iter().collect::<Result<Vec<_>, _>>()?;
    Ok(magnet_links)
}

pub async fn get_a_magnet_link(
    url: &str,
    client: ClientWithMiddleware,
) -> Result<String, CatError> {
    let response = get_response_text(url, client.clone()).await?;
    let resource = Html::parse_document(&response);
    let selector = Selector::parse("a[href]").expect("html element selector must be valid!");
    let magnet_link = resource
        .select(&selector)
        .filter_map(|element| element.value().attr("href"))
        .find(|href| href.starts_with("magnet:"))
        .map(|href| href.to_string())
        .ok_or(CatError::Parse("can not get a magnet link".to_string()))?;
    Ok(magnet_link)
}

pub async fn check_rss_link(url: &str, client: ClientWithMiddleware) -> Result<(), String> {
    let pattern = Regex::new(r"^https?://mikanime\.tv/RSS/Bangumi\?(bangumiId=\d+&subgroupid=\d+|subgroupid=\d+&bangumiId=\d+)$").expect("regex should be valid!");
    if let None = pattern.captures(url) {
        return Err("Invalid url!".into());
    }
    let response = match get_response_text(url, client).await {
        Ok(response) => response,
        Err(error) => return Err(format!("can not visit rss url, error: {}", error).into()),
    };
    match de::from_str::<RSS>(&response){
        Ok(_) => Ok(()),
        Err(error) => Err(format!("can not get correct info from the link, please check bangumiId and subgroupid! Error: {}", error).into())
    }
}

pub async fn rss_receive(
    tx: &mpsc::UnboundedSender<Message>,
    url: &str,
    old_config: &Config,
    client: ClientWithMiddleware,
) -> Result<(), CatError> {
    let response = client.get(url).send().await?.text().await?;
    let rss = de::from_str::<RSS>(&response)?;
    let channel = rss.channel;
    let items = channel.item;
    let latest_item = items
        .first()
        .ok_or(CatError::Parse("can not found latest item!".to_string()))?;
    let parse_link = async || -> Option<(String, String, String, String)> {
        let mut split_ani_sub = channel
            .link
            .split("bangumiId=")
            .nth(1)?
            .split("&subgroupid=");
        let ani_id = split_ani_sub.next()?.to_string();
        let sub_id = split_ani_sub.next()?.to_string();
        let sub_name = get_subgroup_name(&latest_item.link, client).await?;
        let bangumi_name = channel
            .title
            .split(" - ")
            .nth(1)
            .unwrap_or(&channel.title)
            .to_string();
        Some((ani_id, sub_id, sub_name, bangumi_name))
    };
    let (ani_id, sub_id, sub_name, bangumi_name) = parse_link()
        .await
        .ok_or(CatError::Parse("can not found latest item!".to_string()))?;
    let title = format!("[{sub_name}] {bangumi_name}");
    let latest_update = latest_item.torrent.pub_date.clone();
    let bangumi_id = format!("{ani_id}&{sub_id}");
    // check if the bangumi updates and is it first time to be added
    let old_bangumi_dict = &old_config.bangumi;
    let mut magnet_links: Vec<String> = Vec::new();
    if !old_bangumi_dict.contains_key(&bangumi_id) {
        // write to config
        let insert_key = format!("{ani_id}&{sub_id}");
        let cmd = Box::new(|config: &mut Config| {
            config.bangumi.insert(insert_key, latest_update);
        });
        let msg = Message::new(cmd, None);
        tx.send_msg(msg);
        magnet_links
            .extend(get_all_episode_magnet_links(&ani_id, &sub_id, &old_config.filter).await?);
        let insert_title = title.clone();
        let insert_url = url.to_string();
        let cmd = Box::new(|config: &mut Config| {
            config.rss_links.insert(insert_title, insert_url);
        });
        let msg = Message::new(cmd, None);
        tx.send_msg(msg);
    } else if latest_update == old_bangumi_dict[&bangumi_id] {
        println!("{title} 无更新, 上次更新: {latest_update}");
    } else {
        let mut item_iter = items.into_iter().rev();
        for item in &mut item_iter {
            let pub_date = &item.torrent.pub_date;
            if *pub_date == old_bangumi_dict[&bangumi_id] {
                break;
            }
        }
        let new_items = item_iter.collect::<Vec<_>>();
        println!("获取到以下剧集：");
        let filter = &old_config.filter;
        let item_links = filter_episode(&new_items, filter, &sub_id);
        magnet_links = get_all_magnet(item_links).await?;
        let insert_key = format!("{ani_id}&{sub_id}");
        let cmd = Box::new(|config: &mut Config| {
            config.bangumi.insert(insert_key, latest_update);
        });
        let msg = Message::new(cmd, None);
        tx.send_msg(msg);
    }
    if let Some(magnets) = old_config.magnets.get(&title) {
        magnet_links.append(&mut magnets.iter().map(|i| i.clone()).collect::<Vec<_>>());
    }
    if !magnet_links.is_empty() {
        println!(
            "There are some magnet links of {}, let's download them!",
            title
        );
        println!("waiting for cloud download");
        match cloud_download(&magnet_links).await {
            Ok(hash_list) => {
                let mut hash_ani = HashMap::new();
                for i in &hash_list {
                    hash_ani.insert(i.clone(), title.clone());
                }
                let cmd = Box::new(move |config: &mut Config| {
                    config.magnets.remove(&title);
                });
                let msg = Message::new(cmd, None);
                tx.send_msg(msg);
                let notify = Arc::new(Notify::new());
                let cmd = Box::new(|config: &mut Config| {
                    config.hash_ani.extend(hash_ani.into_iter());
                });
                let msg = Message::new(cmd, Some(notify.clone()));
                tx.send_msg(msg);
                notify.notified().await;
            }
            Err(error) => {
                eprintln!("cloud download magnet error: {}", error);
                let cmd = Box::new(move |config: &mut Config| {
                    config
                        .magnets
                        .entry(title)
                        .and_modify(|list| list.append(&mut magnet_links))
                        .or_insert(magnet_links);
                });
                let msg = Message::new(cmd, None);
                tx.send_msg(msg);
                return Err(CloudError::Api("Can not add magnet to cloud!".to_string()))?;
            }
        }
    }
    Ok(())
}

use crate::cloud_manager::cloud_download;
use crate::config_manager::{Bangumi, CONFIG, Config, Message, SafeSend, SubGroup};
use crate::errors::{CatError, CloudError, DownloadError};
use crate::main_proc::{restart_refresh_download, restart_refresh_download_slow};
use crate::time_stamp::TimeStamp;
use crate::{CLIENT_WITH_RETRY, RSS_DATA_PERMIT, TX};
use futures::future::{self, join_all};
use quick_xml::de;
use regex::Regex;
use reqwest::Url;
use reqwest_middleware::ClientWithMiddleware;
use scraper::{Element, Html, Selector};
use serde::Deserialize;
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Arc;
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
    client: &ClientWithMiddleware,
) -> Result<String, DownloadError> {
    let response = client.get(url).send().await?;
    if response.status().is_success() {
        Ok(response.text().await?)
    } else {
        Err(format!("Request failed with status: {}", response.status()))?
    }
}

pub async fn start_rss_receive() -> Result<(), CatError> {
    let permit = RSS_DATA_PERMIT
        .acquire()
        .await
        .expect("This semaphore should be always open");
    // read the config
    let old_config = CONFIG.load_full();
    let urls = old_config
        .rss_links
        .values()
        .map(|(_, url)| url)
        .collect::<Vec<_>>();
    // create the sender futures
    let tx = TX.load_full().ok_or(CatError::Exit)?;
    let mut futs = Vec::new();
    for url in urls {
        futs.push(rss_receive(&tx, url, &old_config, &CLIENT_WITH_RETRY));
    }
    // get the results
    println!("waiting for refreshing rss");
    let results = future::join_all(futs).await;
    println!("rss refresh finished");
    drop(permit);
    for result in results {
        match result {
            Ok(()) => (),
            Err(CatError::Cloud(CloudError::Download(DownloadError::Request(e)))) => {
                eprintln!("{}", e)
            }
            Err(e) => Err(e)?,
        }
    }
    restart_refresh_download().await?;
    restart_refresh_download_slow().await?;
    Ok(())
}

pub fn filter_episode<'a, T: Filter>(
    items: &'a Vec<T>,
    filter: &HashMap<String, SubGroup>,
    sub_id: &str,
) -> Vec<&'a str> {
    let default_filters = &filter["default"].filter_list;
    let mut best_filter: Option<&str> = None;
    let empty_filter: Vec<String> = Vec::new();
    let candidate_filters = match filter.get(sub_id) {
        Some(sub_filters) => sub_filters.filter_list.iter().chain(default_filters.iter()),
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
            .iter()
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
            .iter()
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
    filter: &HashMap<String, SubGroup>,
) -> Result<Vec<String>, CatError> {
    let url = format!(
        "https://mikanime.tv/Home/ExpandEpisodeTable?bangumiId={ani_id}&subtitleGroupId={sub_id}&take=100"
    );
    let response = get_response_text(&url, &CLIENT_WITH_RETRY)
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
                .map(|s| s.to_string())
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

async fn get_subgroup_name(url: &str, client: &ClientWithMiddleware) -> Option<String> {
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
    let futs = item_urls
        .iter()
        .map(|url| get_a_magnet_link(url, &CLIENT_WITH_RETRY))
        .collect::<Vec<_>>();
    let results = join_all(futs).await;
    println!("process links");
    let magnet_links = results.into_iter().collect::<Result<Vec<_>, _>>()?;
    Ok(magnet_links)
}

pub async fn get_a_magnet_link(
    url: &str,
    client: &ClientWithMiddleware,
) -> Result<String, CatError> {
    let response = get_response_text(url, client).await?;
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

pub async fn check_rss_link(url: &str, client: &ClientWithMiddleware) -> Result<(), String> {
    let pattern = Regex::new(r"^https?://mikanime\.tv/RSS/Bangumi\?(bangumiId=\d+&subgroupid=\d+|subgroupid=\d+&bangumiId=\d+)$").expect("regex should be valid!");
    if pattern.captures(url).is_none() {
        return Err("Invalid url!".into());
    }
    let (ani_id, sub_id) = parse_url(url).map_err(|e| e.to_string())?;
    let bangumi_id = format!("{ani_id}&{sub_id}");
    if CONFIG.load().rss_links.contains_key(&bangumi_id) {
        return Err("This rss link already exists!".into());
    }
    let response = match get_response_text(url, client).await {
        Ok(response) => response,
        Err(error) => return Err(format!("can not visit rss url, error: {}", error)),
    };
    match de::from_str::<RSS>(&response) {
        Ok(_) => Ok(()),
        Err(error) => Err(format!(
            "can not get correct info from the link, please check bangumiId and subgroupid! Error: {}",
            error
        )),
    }
}

pub fn parse_url(url: &str) -> Result<(String, String), CatError> {
    let parsed = Url::parse(url)?;
    let (mut ani_id, mut sub_id) = (
        Err(CatError::Parse("missing bangumiId".to_string())),
        Err(CatError::Parse("missing subgroupid".to_string())),
    );
    for (key, value) in parsed.query_pairs() {
        if key == "bangumiId" {
            ani_id = Ok(value.into_owned());
        } else if key == "subgroupid" {
            sub_id = Ok(value.into_owned());
        }
    }
    Ok((ani_id?, sub_id?))
}

pub async fn rss_receive(
    tx: &mpsc::UnboundedSender<Message>,
    url: &str,
    old_config: &Config,
    client: &ClientWithMiddleware,
) -> Result<(), CatError> {
    let response = client.get(url).send().await?.text().await?;
    let rss = de::from_str::<RSS>(&response)?;
    let channel = rss.channel;
    let mut items = channel.item;
    let latest_item = items
        .first()
        .ok_or(CatError::Parse("can not found latest item!".to_string()))?;
    let (ani_id, sub_id) = parse_url(&channel.link)?;
    let latest_update = latest_item.torrent.pub_date.as_str();
    let time_with_tz = format!("{latest_update}+08:00");
    let time = chrono::DateTime::from_str(&time_with_tz)?;
    let latest_update = time.into();
    let latest_episode = latest_item.title.clone();
    let bangumi = Bangumi {
        last_update: latest_update,
        latest_episode: latest_episode.clone(),
    };
    let bangumi_id = format!("{ani_id}&{sub_id}");
    // check if the bangumi updates and is it first time to be added
    let old_bangumi_dict = &old_config.bangumi;
    let mut magnet_links: Vec<String> = Vec::new();
    let update_subgroup_name = async |sub_name: Option<String>| -> Result<(), CatError> {
        if let Some(sub) = old_config.filter.get(&sub_id)
            && sub.name.is_empty()
        {
            let sub_name = match sub_name {
                Some(n) => n,
                None => get_subgroup_name(&latest_item.link, client)
                    .await
                    .ok_or(CatError::Parse("can not get subgroup name!".to_string()))?,
            };
            let id = sub_id.clone();
            let cmd = Box::new(move |config: &mut Config| {
                if let Some(sub) = config.filter.get_mut(&id) {
                    sub.name = sub_name;
                }
            });
            let msg = Message::new(cmd, None);
            tx.send_msg(msg);
        }
        Ok(())
    };
    if !old_bangumi_dict.contains_key(&bangumi_id) {
        magnet_links
            .extend(get_all_episode_magnet_links(&ani_id, &sub_id, &old_config.filter).await?);
        let insert_id = bangumi_id.clone();
        let parse_link = async || -> Option<(String, &str)> {
            let sub_name = get_subgroup_name(&latest_item.link, client).await?;
            let bangumi_name = channel.title.split(" - ").nth(1).unwrap_or(&channel.title);
            Some((sub_name, bangumi_name))
        };
        let (sub_name, bangumi_name) = parse_link()
            .await
            .ok_or(CatError::Parse("can not found latest item!".to_string()))?;
        update_subgroup_name(Some(sub_name.clone())).await?;
        let title = format!("[{sub_name}] {bangumi_name}");
        let insert_title = title.clone();
        let insert_url = url.to_string();
        let cmd = Box::new(|config: &mut Config| {
            config
                .rss_links
                .insert(insert_id, (insert_title, insert_url));
        });
        let msg = Message::new(cmd, None);
        tx.send_msg(msg);
    } else if latest_update <= old_bangumi_dict[&bangumi_id].last_update {
        update_subgroup_name(None).await?;
        let title = &old_config.rss_links[&bangumi_id].0;
        println!("{title} 无更新, 上次更新: {latest_update}");
        println!("\tlatest episode: {}", latest_episode);
        // no need to do anything here, return now!
        return Ok(());
    } else {
        update_subgroup_name(None).await?;
        let mut index = 0;
        for (i, item) in items.iter().enumerate() {
            let pub_date = &item.torrent.pub_date;
            let time_with_tz = format!("{pub_date}+08:00");
            let pub_date: TimeStamp = chrono::DateTime::from_str(&time_with_tz)?.into();
            index = i;
            if pub_date <= old_bangumi_dict[&bangumi_id].last_update {
                break;
            }
        }
        // we have ensured that there is always at least one episode that we have not downloaded,
        // so index >= 1
        items.truncate(index);
        items.reverse();
        println!("获取到以下剧集：");
        let filter = &old_config.filter;
        let item_links = filter_episode(&items, filter, &sub_id);
        magnet_links = get_all_magnet(item_links).await?;
    }
    let title = old_config.rss_links[&bangumi_id].0.clone();
    if let Some(magnets) = old_config.magnets.get(&title) {
        magnet_links.append(&mut magnets.to_vec());
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
    // Update bangumi date. Must be the last step to ensure update process actually occurred.
    let insert_key = bangumi_id.clone();
    let cmd = Box::new(|config: &mut Config| {
        config.bangumi.insert(insert_key, bangumi);
    });
    let msg = Message::new(cmd, None);
    tx.send_msg(msg);
    Ok(())
}

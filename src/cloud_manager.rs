use crate::{
    CLIENT_DOWNLOAD, CLIENT_PROXY, CLIENT_WITH_RETRY, CLIENT_WITH_RETRY_MOBILE,
    config_manager::CONFIG, login_with_qrcode::login_with_qrcode,
};
use anyhow::anyhow;
use indicatif::{ProgressBar, ProgressStyle};
use regex::Regex;
use reqwest::header::{
    CONNECTION, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, HOST, HeaderMap, USER_AGENT,
};
use serde::Deserialize;
use serde_json::Value;
use std::{cell::Cell, collections::HashMap, path::Path};
use tokio::{fs, io::AsyncWriteExt};
use tokio_retry::{Retry, strategy::FixedInterval};

pub const MOBILE_UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 MicroMessenger/8.0.50(0x1800323d) NetType/WIFI Language/zh_CN";

#[derive(Debug, Deserialize)]
pub struct TasksResponse {
    pub page: i32,
    pub page_count: i32,
    pub tasks: Vec<Task>,
}

#[derive(Debug, Deserialize)]
pub struct Task {
    #[serde(rename = "info_hash")]
    pub hash: String,
    #[serde(rename = "percentDone")]
    pub percent_done: i32,
    pub name: String,
    pub status: i32,
}

#[derive(Debug, Deserialize)]
pub struct CloudDownloadResponse {
    pub result: Vec<CloudDownloadResult>,
}

#[derive(Debug, Deserialize)]
pub struct CloudDownloadResult {
    pub errcode: i32,
    #[serde(rename = "info_hash")]
    pub hash: Option<String>,
    pub url: String,
}

pub fn extract_magnet_hash(link: &str) -> Option<String> {
    let re = Regex::new(r"magnet:\?xt=urn:btih:([0-9a-fA-F]{40}|[A-Z2-7]{32})").unwrap();
    re.captures(link)
        .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
}

pub async fn get_cloud_cookies() -> String {
    let try_times = Cell::new(0);
    match Retry::spawn(FixedInterval::from_millis(5000).take(5), async || {
        if try_times.get() > 0 {
            eprintln!("Failed to login, waiting for retry.");
        }
        try_times.set(1);
        login_with_qrcode("alipaymini").await
    })
    .await
    {
        Ok(cookies) => cookies,
        Err(error) => {
            eprintln!("Can not get cookies after retries!\nError: {error}");
            std::process::exit(1);
        }
    }
}

pub async fn download_file(url: &str, path: &Path) -> Result<(), anyhow::Error> {
    let client = CLIENT_DOWNLOAD.clone();
    let mut response = client.get(url).send().await?;
    let content_length = response
        .headers()
        .get(CONTENT_LENGTH)
        .ok_or_else(|| anyhow::Error::msg("Can not download"))?
        .to_str()?
        .parse::<u64>()?;
    let progress = ProgressBar::new(content_length);
    progress.set_style(
        ProgressStyle::default_bar()
            .template("{wide_bar} {bytes} / {total_bytes} ({binary_bytes_per_sec}  eta: {eta})")?,
    );
    fs::create_dir_all(path.parent().unwrap()).await?;
    let mut file = fs::File::create(path).await?;
    while let Some(data) = response.chunk().await? {
        file.write_all(&data).await?;
        progress.inc(data.len() as u64);
    }
    progress.finish();
    println!("finished!");
    Ok(())
}

pub async fn cloud_download(urls: &[String]) -> Result<Vec<String>, anyhow::Error> {
    let client = CLIENT_WITH_RETRY_MOBILE.clone();
    let config_lock = CONFIG.read().await;
    let cookies = config_lock.get_value()["cookies"].as_str().unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "115.com".parse().unwrap());
    headers.insert(CONNECTION, "keep-alive".parse().unwrap());
    headers.insert(
        CONTENT_TYPE,
        "application/x-www-form-urlencoded".parse().unwrap(),
    );
    headers.insert(COOKIE, cookies.parse().unwrap());
    let params = serde_json::json!({
        "ct": "lixian",
        "ac": "add_task_urls",
    });
    let mut data = HashMap::new();
    for (index, url) in urls.iter().enumerate() {
        data.insert(format!("url[{index}]"), url);
    }
    let response = client
        .post("https://115.com/web/lixian/")
        .headers(headers)
        .query(&params)
        .form(&data)
        .send()
        .await?
        .text()
        .await?;
    let response: CloudDownloadResponse = serde_json::from_str(&response)?;
    let download_result = response.result;
    let result = download_result
        .into_iter()
        .map(|i| match i.hash {
            Some(hash) => hash,
            None => extract_magnet_hash(&i.url).unwrap(),
        })
        .collect();
    Ok(result)
}

pub async fn del_cloud_task(hash: &str) -> Result<(), anyhow::Error> {
    let client = CLIENT_WITH_RETRY.clone();
    let config_lock = CONFIG.read().await;
    let cookies = config_lock.get_value()["cookies"].as_str().unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "115.com".parse().unwrap());
    headers.insert(CONNECTION, "keep-alive".parse().unwrap());
    headers.insert(
        CONTENT_TYPE,
        "application/x-www-form-urlencoded; charset=UTF-8"
            .parse()
            .unwrap(),
    );
    headers.insert(COOKIE, cookies.parse().unwrap());
    let params = serde_json::json!({
        "ct": "lixian",
        "ac": "task_del",
    });
    let uid = cookies
        .split(";")
        .next()
        .unwrap()
        .split("=")
        .nth(1)
        .unwrap()
        .split("_")
        .next()
        .unwrap();
    let data = serde_json::json!({
        "hash[0]": hash,
        "uid": uid,
    });
    let response = client
        .post("https://115.com/web/lixian/")
        .headers(headers)
        .query(&params)
        .form(&data)
        .send()
        .await?
        .text()
        .await?;
    let response_json: Value = serde_json::from_str(&response)?;
    if response_json["state"] == true {
        Ok(())
    } else {
        Err(anyhow::anyhow!(response))
    }
}

pub async fn get_tasks_list(hash_list: Vec<&String>) -> Result<Vec<Task>, anyhow::Error> {
    // let client = CLIENT_WITH_RETRY.clone();
    let client = CLIENT_WITH_RETRY_MOBILE.clone();
    // let client = CLIENT_PROXY.clone();
    let config_lock = CONFIG.read().await;
    let cookies = config_lock.get_value()["cookies"].as_str().unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "115.com".parse().unwrap());
    headers.insert(CONNECTION, "keep-alive".parse().unwrap());
    headers.insert(COOKIE, cookies.parse().unwrap());
    headers.insert(USER_AGENT, MOBILE_UA.parse().unwrap());
    let mut params = serde_json::json!({
        "ct": "lixian",
        "ac": "task_lists",
        "page": 1,
    });
    let response = client
        .post("https://115.com/web/lixian/")
        .headers(headers.clone())
        .query(&params)
        .send()
        .await?
        .text()
        .await?;
    let tasks_response: TasksResponse = match serde_json::from_str(&response) {
        Ok(value) => value,
        Err(_) => {
            return Err(anyhow!(
                "Can not get valid tasks list! Error response: {response}"
            ));
        }
    };
    let mut pages = tasks_response.page_count;
    let mut current_tasks = tasks_response
        .tasks
        .into_iter()
        .filter(|task| hash_list.iter().any(|hash| **hash == task.hash))
        .collect::<Vec<_>>();
    let mut page = 1;
    println!("checked page 1, now start checking all tasks");
    while current_tasks.len() < hash_list.len() && page < pages {
        page += 1;
        println!("page: {}", page);
        params["page"] = page.into();
        let response = client
            .post("https://115.com/web/lixian/")
            .headers(headers.clone())
            .query(&params)
            .send()
            .await?
            .text()
            .await?;
        let tasks_response: TasksResponse = match serde_json::from_str(&response) {
            Ok(value) => value,
            Err(_) => {
                return Err(anyhow!(
                    "Can not get valid tasks list! Error response: {response}"
                ));
            }
        };
        pages = tasks_response.page_count;
        let mut left_tasks = tasks_response
            .tasks
            .into_iter()
            .filter(|task| hash_list.iter().any(|hash| **hash == task.hash))
            .collect::<Vec<_>>();
        current_tasks.append(&mut left_tasks);
    }
    Ok(current_tasks)
}

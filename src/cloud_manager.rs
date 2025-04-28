use crate::{
    CLIENT_DOWNLOAD, CLIENT_WITH_RETRY, config_manager::CONFIG,
    login_with_qrcode::login_with_qrcode,
};
use indicatif::{ProgressBar, ProgressStyle};
use reqwest::header::{CONNECTION, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, HOST, HeaderMap};
use serde_json::Value;
use std::{cell::Cell, collections::HashMap, path::Path};
use tokio::{fs, io::AsyncWriteExt};
use tokio_retry::{Retry, strategy::FixedInterval};

pub const MOBILE_UA: &str = "Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Mobile/15E148 MicroMessenger/8.0.50(0x1800323d) NetType/WIFI Language/zh_CN";

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
    let client = CLIENT_WITH_RETRY.clone();
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
    let response_json: Value = serde_json::from_str(&response)?;
    if response_json["errcode"] == 0 || response_json["errcode"] == 10008 {
        let result = response_json["result"]
            .as_array()
            .ok_or_else(|| anyhow::Error::msg("can not get result"))?
            .iter()
            .map(|i| i["info_hash"].as_str().unwrap().to_string())
            .collect();
        Ok(result)
    } else {
        Err(anyhow::anyhow!(response))
    }
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
    println!("{}", uid);
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
        println!("{:?}", response_json);
        Ok(())
    } else {
        Err(anyhow::anyhow!(response))
    }
}

pub async fn get_tasks_list() -> Result<Vec<Value>, anyhow::Error> {
    let client = CLIENT_WITH_RETRY.clone();
    let config_lock = CONFIG.read().await;
    let cookies = config_lock.get_value()["cookies"].as_str().unwrap();
    let downloading_dict = &config_lock.get_value()["downloading_hash"];
    let hash_list = downloading_dict.as_array().unwrap();
    let mut headers = HeaderMap::new();
    headers.insert(HOST, "115.com".parse().unwrap());
    headers.insert(CONNECTION, "keep-alive".parse().unwrap());
    headers.insert(COOKIE, cookies.parse().unwrap());
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
    let mut response_json: Value = serde_json::from_str(&response)?;
    // println!("{}", response_json);
    let pages = response_json["page_count"]
        .as_i64()
        .ok_or_else(|| anyhow::Error::msg("Can not find page_count"))?;
    let tasks = response_json["tasks"]
        .as_array_mut()
        .ok_or_else(|| anyhow::Error::msg("Can not get tasks list"))?;
    let mut current_tasks = tasks
        .iter_mut()
        .filter(|task| hash_list.iter().any(|hash| *hash == task["info_hash"]))
        .map(|task| task.take())
        .collect::<Vec<_>>();
    let mut page: i64 = 1;
    while current_tasks.len() < hash_list.len() && page <= pages {
        page += 1;
        params.as_object_mut().unwrap()["page"] = page.into();
        let response = client
            .post("https://115.com/web/lixian/")
            .headers(headers.clone())
            .query(&params)
            .send()
            .await?
            .text()
            .await?;
        let mut response_json: Value = serde_json::from_str(&response)?;
        let mut tasks_value = response_json["tasks"].take();
        let tasks = tasks_value
            .as_array_mut()
            .ok_or_else(|| anyhow::Error::msg("Can not get tasks list"))?;
        let mut left_tasks = tasks
            .iter_mut()
            .filter(|task| hash_list.iter().any(|hash| *hash == task["info_hash"]))
            .map(|task| task.take())
            .collect::<Vec<_>>();
        current_tasks.append(&mut left_tasks);
    }
    Ok(current_tasks)
}

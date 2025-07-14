use crate::{
    CLIENT_DOWNLOAD, CLIENT_WITH_RETRY, CLIENT_WITH_RETRY_MOBILE, TX,
    cloud::download::{DownloadInfo, FileDownloadUrl, get_download_link},
    config_manager::{CONFIG, Config, Message, SafeSend},
    errors::{CatError, CloudError, DownloadError},
    login_with_qrcode::login_with_qrcode,
};
use futures::future::join_all;
use indicatif::{ProgressBar, ProgressStyle};
use regex::Regex;
use reqwest::{
    Client,
    header::{CONNECTION, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, HOST, HeaderMap, HeaderValue},
};
use reqwest_middleware::ClientWithMiddleware;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    cell::Cell,
    collections::HashMap,
    os::unix::fs::FileExt,
    path::{Path, PathBuf},
    sync::Arc,
};
use std::{fs as sfs, str::FromStr};
use tokio::{fs, sync::Notify};
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
    /// `folder_id` is the id of the folder itself or the id of the file's parent folder
    #[serde(rename = "file_id")]
    pub folder_id: String,
    #[serde(rename = "delete_file_id")]
    pub file_id: String,
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

#[derive(Debug, Deserialize)]
pub struct FileInfo {
    /// `folder_id` is the id of the folder itself or the id of the file's parent folder
    #[serde(rename = "cid")]
    pub folder_id: String,
    /// only file has `file_id`
    #[serde(rename = "fid")]
    pub file_id: Option<String>,
    #[serde(rename = "n")]
    pub name: String,
    #[serde(rename = "pc")]
    pub pick_code: String,
}

#[derive(Debug, Deserialize)]
pub struct FileInfoResponse {
    #[serde(rename = "file_name")]
    pub name: String,
}

pub struct FileWithPath {
    pub info: FileInfo,
    pub path: PathBuf,
}

#[derive(Debug, Deserialize)]
pub struct FileListResponse {
    pub count: i32,
    #[serde(rename = "data")]
    pub files: Vec<FileInfo>,
}

#[derive(Deserialize)]
pub struct Errors {
    #[serde(rename = "error")]
    pub msg: String,
    #[serde(rename = "errNo")]
    pub error_no: i32,
}

pub fn extract_magnet_hash(link: &str) -> Option<String> {
    let re =
        Regex::new(r"xt=urn:btih:([a-fA-F0-9]{40}|[A-Z2-7]{32})").expect("regex should be valid!");
    re.captures(link)
        .and_then(|caps| caps.get(1).map(|m| m.as_str().to_string()))
}

pub async fn get_cloud_cookies() -> Result<String, CatError> {
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
        Ok(cookies) => Ok(cookies),
        Err(error) => Err(CatError::GetCookie(format!(
            "Can not get cookies after retries!\nError: {error}"
        ))),
    }
}

async fn download_chunk(
    url: &str,
    client: Client,
    start: u64,
    end: u64,
    file: &sfs::File,
    progress: &ProgressBar,
) -> Result<(), DownloadError> {
    let range_header = format!("bytes={}-{}", start, end);
    let mut response = client.get(url).header("Range", range_header).send().await?;
    let mut current: u64 = start;
    while let Some(chunk) = response.chunk().await? {
        file.write_all_at(&chunk, current)?;
        let delta = chunk.len() as u64;
        progress.inc(delta);
        current += delta;
    }
    Ok(())
}

pub async fn download_file(url: &str, path: &Path) -> Result<(), DownloadError> {
    let client = CLIENT_DOWNLOAD.clone();
    let response = client.head(url).send().await?;
    let content_length = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|s| s.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or(DownloadError::ContentLength(
            "Can not get CONTENT_LENGTH".to_string(),
        ))?;
    let average = content_length / 2;
    println!("{} {}", content_length, average);
    let progress = ProgressBar::new(content_length);
    progress.set_style(
        ProgressStyle::default_bar()
            .template("{wide_bar} {bytes} / {total_bytes} ({binary_bytes_per_sec}  eta: {eta})")
            .expect("progress style should be valid!"),
    );
    let mut current: u64 = 0;
    fs::create_dir_all(path.parent().ok_or(DownloadError::Path(
        "path's parent folder is missing".to_string(),
    ))?)
    .await?;
    let file = sfs::File::create(path)?;
    let mut futs = Vec::new();
    futs.push(download_chunk(
        url,
        client.clone(),
        current,
        current + average,
        &file,
        &progress,
    ));
    current += average;
    futs.push(download_chunk(
        url,
        client.clone(),
        current,
        content_length,
        &file,
        &progress,
    ));
    let results = join_all(futs).await;
    progress.finish();
    for i in results {
        i?;
    }
    println!("finished!");
    Ok(())
}

pub async fn cloud_download(urls: &[String]) -> Result<Vec<String>, CloudError> {
    let client = CLIENT_WITH_RETRY_MOBILE.clone();
    let cookies = &CONFIG.load().cookies;
    let mut headers = HeaderMap::new();
    let insert_headers = |map: &mut HeaderMap| -> Result<(), <HeaderValue as FromStr>::Err> {
        map.insert(HOST, "115.com".parse()?);
        map.insert(CONNECTION, "keep-alive".parse()?);
        map.insert(CONTENT_TYPE, "application/x-www-form-urlencoded".parse()?);
        Ok(())
    };
    insert_headers(&mut headers).expect("headers should be valid!");
    headers.insert(COOKIE, cookies.parse()?);
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
            Some(hash) => Ok(hash),
            None => extract_magnet_hash(&i.url)
                .ok_or(CloudError::Param("invalid magnet link".to_string())),
        })
        .collect::<Result<Vec<String>, CloudError>>()?;
    Ok(result)
}

pub async fn del_cloud_task(hash: &str) -> Result<(), CloudError> {
    let client = CLIENT_WITH_RETRY.clone();
    let cookies = &CONFIG.load().cookies;
    let mut headers = HeaderMap::new();
    let insert_headers = |map: &mut HeaderMap| -> Result<(), <HeaderValue as FromStr>::Err> {
        map.insert(HOST, "115.com".parse()?);
        map.insert(CONNECTION, "keep-alive".parse()?);
        map.insert(CONTENT_TYPE, "application/x-www-form-urlencoded".parse()?);
        Ok(())
    };
    insert_headers(&mut headers).expect("headers should be valid!");
    headers.insert(COOKIE, cookies.parse()?);
    let params = serde_json::json!({
        "ct": "lixian",
        "ac": "task_del",
    });
    let uid = cookies
        .split(";")
        .next()
        .and_then(|s| s.split("=").nth(1))
        .and_then(|s| s.split("_").next())
        .ok_or(CloudError::CookiesParse("invalid cookies!".to_string()))?;
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
        Err(response.into())
    }
}

pub async fn get_tasks_list(hash_list: Vec<&String>) -> Result<Vec<Task>, CloudError> {
    let client = CLIENT_WITH_RETRY.clone();
    let cookies = &CONFIG.load().cookies;
    let mut headers = HeaderMap::new();
    let insert_headers = |map: &mut HeaderMap| -> Result<(), <HeaderValue as FromStr>::Err> {
        map.insert(HOST, "115.com".parse()?);
        map.insert(CONNECTION, "keep-alive".parse()?);
        Ok(())
    };
    insert_headers(&mut headers).expect("headers should be valid!");
    headers.insert(COOKIE, cookies.parse()?);
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
            return Err(CloudError::Api(format!(
                "Can not get valid tasks list! Error response: {response}"
            )));
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
                return Err(CloudError::Api(format!(
                    "Can not get valid tasks list! Error response: {response}"
                )));
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

pub async fn list_files(
    client: ClientWithMiddleware,
    folder_id: &str,
    offset: i32,
    limit: i32,
) -> Result<FileListResponse, CloudError> {
    let params = json!(
        {
            "aid": "1",
            "cid": folder_id,
            // Order
            "o": "file_name",
            // is ascend order?
            "asc": "1",
            "offset": offset,
            "show_dir": "1",
            "limit": limit,
            "code": "",
            "scid": "",
            "snap": "0",
            "natsort": "0",
            "record_open_time": "1",
            "count_folders": "1",
            "type": "",
            "source": "",
            "format": "json",
            "fc_mix": "0",
        }
    );
    let cookies = &CONFIG.load().cookies;
    let mut headers = HeaderMap::new();
    headers.insert(COOKIE, cookies.parse()?);
    let response = client
        .get("https://webapi.115.com/files")
        .headers(headers)
        .query(&params)
        .send()
        .await?
        .text()
        .await?;
    let result = serde_json::from_str::<FileListResponse>(&response).or_else(|e| {
        let mut errors = format!("list_files: can not serialize list file response, error: {e}\n");
        match serde_json::from_str::<Errors>(&response) {
            Ok(error) => {
                errors.push_str(&format!(
                    "list_files: Error No: {}, Error message: {}",
                    error.error_no, error.msg
                ));
            }
            Err(e) => errors.push_str(&format!("list_files: can not get errors, error: {}", e)),
        }
        Err(errors)
    })?;

    Ok(result)
}

pub async fn list_all_files(
    client: ClientWithMiddleware,
    folder_id: &str,
) -> Result<Vec<FileInfo>, CloudError> {
    let response = list_files(client.clone(), &folder_id, 0, 20).await?;
    let file_count = response.count;
    let mut current = response.files.len() as i32;
    let mut files = response.files;
    while current < file_count {
        let mut response =
            list_files(client.clone(), &folder_id, current, file_count - current).await?;
        current += response.files.len() as i32;
        files.append(&mut response.files);
    }
    Ok(files)
}

pub async fn download_a_folder(folder_id: &str, ani_name: Option<&str>) -> Result<(), CloudError> {
    let client = CLIENT_WITH_RETRY.clone();
    let mut storge_path = PathBuf::new();
    storge_path.push("downloads/115");
    match ani_name {
        Some(name) => storge_path.push(name),
        None => {
            let result = get_file_info(client.clone(), folder_id).await?;
            storge_path.push(result.name);
        }
    }
    let mut files = list_all_files(client.clone(), folder_id)
        .await?
        .into_iter()
        .map(|info| FileWithPath {
            info,
            path: PathBuf::new(),
        })
        .collect::<Vec<_>>();
    while let Some(file) = files.pop() {
        match file.info.file_id {
            Some(_) => {
                let DownloadInfo {
                    file_name,
                    url: FileDownloadUrl { url, .. },
                    ..
                } = get_download_link(client.clone(), file.info.pick_code).await?;
                let mut path = storge_path.clone();
                path.push(file.path);
                path.push(&file_name);
                download_file(&url, &path).await?;
            }
            None => {
                let mut new_files = list_all_files(client.clone(), &file.info.folder_id)
                    .await?
                    .into_iter()
                    .map(|info| {
                        let mut file_with_path = FileWithPath {
                            info,
                            path: PathBuf::new(),
                        };
                        file_with_path.path.push(&file.path);
                        file_with_path.path.push(&file.info.name);
                        file_with_path
                    })
                    .collect::<Vec<_>>();
                files.append(&mut new_files);
            }
        }
    }
    Ok(())
}

pub async fn check_cookies() -> Result<(), CatError> {
    let client = CLIENT_WITH_RETRY.clone();
    match list_files(client, "0", 0, 1).await {
        Ok(_) => {}
        Err(e) => {
            if let CloudError::Api(_) = e {
                println!("Cookies is expired, try to update...");
                let cookies = get_cloud_cookies().await?;
                let tx = TX
                    .load()
                    .as_deref()
                    .ok_or(CatError::Exit("exiting now...".to_string()))?
                    .clone();
                let notify = Arc::new(Notify::new());
                let cmd = Box::new(|config: &mut Config| {
                    config.cookies = cookies;
                });
                let msg = Message::new(cmd, Some(notify.clone()));
                tx.send_msg(msg);
                notify.notified().await;
                println!("Cookies is now up to date!");
            } else {
                Err(e)?
            }
        }
    }
    Ok(())
}

pub async fn get_file_info(
    client: ClientWithMiddleware,
    folder_id: &str,
) -> Result<FileInfoResponse, CloudError> {
    let params = json!({
        "cid": folder_id
    });
    let cookies = &CONFIG.load().cookies;
    let mut headers = HeaderMap::new();
    headers.insert(COOKIE, cookies.parse()?);
    let response = client
        .get("https://webapi.115.com/category/get")
        .headers(headers)
        .query(&params)
        .send()
        .await?
        .text()
        .await?;
    let result = serde_json::from_str::<FileInfoResponse>(&response).or_else(|e| {
        let mut errors =
            format!("get file info: can not serialize list file response, error: {e}\n");
        match serde_json::from_str::<Errors>(&response) {
            Ok(error) => {
                errors.push_str(&format!(
                    "get file info: Error No: {}, Error message: {}",
                    error.error_no, error.msg
                ));
            }
            Err(e) => errors.push_str(&format!("get file info: can not get errors, error: {}", e)),
        }
        Err(errors)
    })?;
    Ok(result)
}

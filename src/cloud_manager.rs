use crate::cloud::download::{DownloadInfo, FileDownloadUrl, get_download_link};
use crate::config_manager::{CONFIG, SafeSend};
use crate::drop_guard::DropGuard;
use crate::errors::{CatError, CloudError, DownloadError};
use crate::id::Id;
use crate::login_with_qrcode::login_with_qrcode;
use crate::socket_utils::{DownloadMsg, DownloadState, ServerMsg};
use crate::{
    BROADCAST_TX, CLIENT_DOWNLOAD, CLIENT_WITH_RETRY, CLIENT_WITH_RETRY_MOBILE, LOGIN_STATUS,
};
use futures::future::join_all;
use regex::Regex;
use reqwest::Client;
use reqwest::header::{
    CONNECTION, CONTENT_LENGTH, CONTENT_TYPE, COOKIE, HOST, HeaderMap, HeaderValue,
};
use reqwest_middleware::ClientWithMiddleware;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs as sfs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use tokio::fs;
use tokio::sync::Semaphore;
use tokio_retry::Retry;
use tokio_retry::strategy::FixedInterval;

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
    /// only file has hash
    #[serde(rename = "sha")]
    pub sha1: Option<String>,
    #[serde(rename = "s")]
    pub size: Option<u64>,
    #[serde(rename = "pc")]
    pub pick_code: String,
}

#[derive(Debug, Deserialize)]
pub struct FileInfoResponse {
    #[serde(rename = "file_name")]
    pub name: String,
}

#[derive(Debug)]
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
    let try_times = AtomicBool::new(false);
    match Retry::spawn(FixedInterval::from_millis(5000).take(2), async || {
        if try_times.load(std::sync::atomic::Ordering::Relaxed) {
            eprintln!("waiting for retry...");
        }
        try_times.store(true, std::sync::atomic::Ordering::Relaxed);
        login_with_qrcode("tv").await.inspect_err(|e| {
            eprintln!("Login with qrcode failed, error: {e}");
        })
    })
    .await
    {
        Ok(cookies) => Ok(cookies),
        Err(error) => Err(CatError::GetCookie(format!(
            "Can not get cookies after retries!\nError: {error}"
        ))),
    }
}

/// **NOTE:**
/// We may need to reconsider multi-threaded (parallel range) downloads for a single file.
/// Some CDN or object storage providers appear to enforce per-file session limits
/// or anti-abuse policies that reject concurrent Range requests.
///
/// In certain cases, when one range request completes, the CDN may treat the
/// download session as finished and actively reject the remaining in-flight
/// ranges with HTTP 403 responses.
///
/// Since this behavior depends on CDN implementation details and cannot be
/// reliably detected or controlled client-side, it might be safer to fall back
/// to a single-connection download instead of parallel chunk downloads for
/// individual files.
///
/// We may add resume support later...
async fn download_single(
    url: &str,
    client: &Client,
    file: &mut sfs::File,
    id: Id,
) -> Result<(), DownloadError> {
    use std::io::Write;
    let mut response = client.get(url).send().await?;
    while let Some(chunk) = response.chunk().await? {
        file.write_all(&chunk)?;
        let msg = ServerMsg::Download(DownloadMsg {
            id,
            state: DownloadState::Downloading(chunk.len() as u64),
        });
        BROADCAST_TX.send_msg(msg);
    }
    Ok(())
}

pub async fn download_file(
    url: &str,
    path: &Path,
    id: Id,
    size: u64,
    mut hash: String,
) -> Result<(), DownloadError> {
    let client = &CLIENT_DOWNLOAD;
    let response = client.head(url).send().await?;
    let content_length = response
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|s| s.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
        .ok_or(DownloadError::ContentLength(
            "Can not get CONTENT_LENGTH".to_string(),
        ))?;
    if content_length != size {
        return Err(DownloadError::ContentLength(
            "inconsistent content length".to_string(),
        ));
    }
    fs::create_dir_all(path.parent().ok_or(DownloadError::Path(
        "path's parent folder is missing".to_string(),
    ))?)
    .await?;
    // if the file exists, delete it first
    if sfs::exists(path)? {
        sfs::remove_file(path)?;
    }
    let mut file = sfs::File::create(path)?;
    download_single(url, client, &mut file, id).await?;
    let sha1 = sha1_of_file(path)?;
    // `sha1` is upper case, ensure `hash` is upper case, too.
    hash.make_ascii_uppercase();
    if sha1 != hash {
        return Err(DownloadError::Hash {
            expected: hash,
            found: sha1,
        });
    }
    // sync_all() is slow but ensures data is fully flushed to disk.
    // It is only used together with hash verification to provide
    // stronger integrity guarantees when needed.
    file.sync_all()?;

    let msg = ServerMsg::Download(DownloadMsg {
        id,
        state: DownloadState::Finished,
    });
    BROADCAST_TX.send_msg(msg);
    println!("{:?} finished!", path);
    Ok(())
}

pub fn sha1_of_file(path: &Path) -> std::io::Result<String> {
    use sha1::{Digest, Sha1};
    use std::io::Read;

    let mut file = sfs::File::open(path)?;
    let mut hasher = Sha1::new();
    let mut buffer = [0u8; 8192];

    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    let result = hasher.finalize();
    Ok(format!("{:X}", result))
}

pub async fn cloud_download(urls: &[String]) -> Result<Vec<String>, CloudError> {
    let client = &CLIENT_WITH_RETRY_MOBILE;
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
    let client = &CLIENT_WITH_RETRY;
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
        .ok_or(CloudError::Cookies("invalid cookies!".to_string()))?;
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
    let client = &CLIENT_WITH_RETRY;
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
    client: &ClientWithMiddleware,
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
    let result = serde_json::from_str::<FileListResponse>(&response).map_err(|e| {
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
        errors
    })?;

    Ok(result)
}

pub async fn list_all_files(
    client: &ClientWithMiddleware,
    folder_id: &str,
) -> Result<Vec<FileInfo>, CloudError> {
    let response = list_files(client, folder_id, 0, 20).await?;
    let file_count = response.count;
    let mut current = response.files.len() as i32;
    let mut files = response.files;
    while current < file_count {
        let mut response = list_files(client, folder_id, current, file_count - current).await?;
        current += response.files.len() as i32;
        files.append(&mut response.files);
    }
    Ok(files)
}

pub async fn download_a_folder(folder_id: &str, ani_name: Option<&str>) -> Result<(), CloudError> {
    let client = &CLIENT_WITH_RETRY;
    let mut storge_path = PathBuf::new();
    storge_path.push("downloads/115");
    match ani_name {
        Some(name) => storge_path.push(name),
        None => {
            let result = get_file_info(client, folder_id).await?;
            storge_path.push(result.name);
        }
    }
    let mut files = list_all_files(client, folder_id)
        .await?
        .into_iter()
        .map(|info| FileWithPath {
            info,
            path: PathBuf::new(),
        })
        .collect::<Vec<_>>();
    println!("get all files success!");
    let mut files_to_download = Vec::new();
    while let Some(file) = files.pop() {
        match file.info.file_id {
            Some(_) => {
                let id = Id::generate();
                let msg = ServerMsg::Download(DownloadMsg {
                    id,
                    state: DownloadState::Start(Box::new((
                        file.info.name.clone(),
                        file.info.size.unwrap(),
                    ))),
                });
                BROADCAST_TX.send_msg(msg);
                let bar_guard = DropGuard::new(id, |id| {
                    let msg = ServerMsg::Download(DownloadMsg {
                        id,
                        state: DownloadState::Failed,
                    });
                    BROADCAST_TX.send_msg(msg);
                });
                files_to_download.push((bar_guard, file));
            }
            None => {
                // wait for a while to avoid getting banned
                tokio::time::sleep(Duration::from_secs(1)).await;
                let mut new_files = list_all_files(client, &file.info.folder_id)
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
    // restrict parallel downloading tasks
    let sema = Arc::new(Semaphore::new(5));
    let mut download_handles = Vec::new();
    for (id_guard, file) in files_to_download {
        // wait for a while to avoid getting banned
        tokio::time::sleep(Duration::from_secs(1)).await;
        let DownloadInfo {
            file_name,
            url: FileDownloadUrl { url, .. },
            ..
        } = get_download_link(client, file.info.pick_code).await?;
        let mut path = storge_path.clone();
        path.push(file.path);
        path.push(&file_name);
        let sema = sema.clone();
        // require the permission first, to avoid holding the
        // download url for a long time
        let permit = sema
            .acquire_owned()
            .await
            .expect("semaphore is not closed here");
        download_handles.push(tokio::spawn(async move {
            let _permit = permit;
            let id = *id_guard.inner();
            (
                id_guard,
                download_file(
                    &url,
                    &path,
                    id,
                    file.info.size.unwrap(),
                    file.info.sha1.unwrap(),
                )
                .await,
            )
        }));
    }
    let download_errors = join_all(download_handles)
        .await
        .into_iter()
        .filter_map(|result| {
            let (id, res) = result.expect("task is not cancelled or panicked");
            match res {
                Ok(()) => {
                    id.into_inner();
                    None
                }
                Err(e) => Some((id.into_inner(), e)),
            }
        })
        .collect::<Vec<_>>();
    if !download_errors.is_empty() {
        return Err(CloudError::DownloadErrors(download_errors));
    }
    Ok(())
}

pub async fn check_cookies() -> Result<(), CatError> {
    match list_files(&CLIENT_WITH_RETRY, "0", 0, 1).await {
        Ok(_) => {
            LOGIN_STATUS.store(true, std::sync::atomic::Ordering::Relaxed);
        }
        Err(CloudError::Api(_)) => {
            println!("Cookies is expired, please login again");
            LOGIN_STATUS.store(false, std::sync::atomic::Ordering::Relaxed);
        }
        error => {
            error?;
        }
    }
    Ok(())
}

pub async fn get_file_info(
    client: &ClientWithMiddleware,
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
    let result = serde_json::from_str::<FileInfoResponse>(&response).map_err(|e| {
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
        errors
    })?;
    Ok(result)
}

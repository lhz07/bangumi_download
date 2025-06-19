use std::{
    path::PathBuf,
    sync::Arc,
};

use anyhow::anyhow;
use once_cell::sync::Lazy;
use reqwest::header::{AUTHORIZATION, HeaderMap};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::sync::{Notify, RwLock};

use crate::{
    cloud_manager::{download_file, get_cloud_cookies}, config_manager::{Message, MessageCmd, MessageType, CONFIG}, CLIENT, TX
};
static TOKEN: Lazy<RwLock<String>> = Lazy::new(|| RwLock::new(String::new()));

pub async fn get_alist_name_passwd() -> (String, String) {
    let mut name = String::new();
    let mut password;
    loop {
        println!("Username:");
        std::io::stdin()
            .read_line(&mut name)
            .expect("Failed to read username!");
        name = name.trim().to_string();
        let mut hasher = Sha256::new();
        println!("Password:");
        hasher.update(
            rpassword::read_password().expect("Failed to read password")
                + "-https://github.com/alist-org/alist",
        );
        password = hex::encode(hasher.finalize());
        match get_alist_token(&name, &password).await {
            Ok(_) => break,
            Err(error) => eprintln!("{}", error),
        }
    }
    (name, password)
}

pub async fn get_alist_token(username: &str, password: &str) -> Result<(), anyhow::Error> {
    let client = CLIENT.clone();
    let body = serde_json::json!({
        "username": username,
        "password": password,
    });
    let response = client
        .post("http://127.0.0.1:5244/api/auth/login/hash")
        .json(&body)
        .send()
        .await?
        .text()
        .await?;
    let response_json: Value = serde_json::from_str(&response)?;
    if response_json["code"] == 200 {
        let mut token = TOKEN.write().await;
        token.clear();
        token.push_str(response_json["data"]["token"].as_str().unwrap());
    } else {
        return Err(anyhow!(
            response_json["message"]
                .as_str()
                .unwrap_or("Wrong username or password")
                .to_string()
        )); // Return the error
    }
    // println!("{}", TOKEN.read()?);
    Ok(())
}

pub async fn get_file_raw_url(path: &str) -> Result<(String, String), anyhow::Error> {
    let json_data = serde_json::json!({
        "path": path,
        "password": ""
    });
    let client = CLIENT.clone();
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, TOKEN.read().await.parse().unwrap());
    let response = client
        .post("http://127.0.0.1:5244/api/fs/get")
        .headers(headers)
        .json(&json_data)
        .send()
        .await?
        .text()
        .await?;
    let response_json: Value = serde_json::from_str(&response)?;
    if response_json["code"] == 200 {
        let name = response_json["data"]["name"]
            .as_str()
            .ok_or_else(|| anyhow::Error::msg("Can not find file name!"))?
            .to_string();
        let raw_url = response_json["data"]["raw_url"]
            .as_str()
            .ok_or_else(|| anyhow::Error::msg("Can not find file url!"))?
            .to_string();
        Ok((name, raw_url))
    } else {
        Err(anyhow!(
            response_json["message"].as_str().unwrap().to_string()
        ))
    }
}

pub async fn check_cookies() -> Result<(), anyhow::Error> {
    let client = CLIENT.clone();
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, TOKEN.read().await.parse().unwrap());
    let response = client
        .get("http://127.0.0.1:5244/api/admin/storage/get?id=1")
        .headers(headers)
        .send()
        .await?
        .text()
        .await?;
    let response_json: Value = serde_json::from_str(&response)?;
    println!("{}", response_json["data"]["status"]);
    if response_json["data"]["status"].as_str().unwrap_or("error") != "work" {
        eprintln!("{}", response_json["data"]["status"]);
        println!("Cookies is expired, try to update...");
        let cookies = get_cloud_cookies().await;
        let tx = TX.read().await.clone().unwrap();
        let notify = Arc::new(Notify::new());
        let msg = Message::new(
            vec!["cookies".to_string()],
            MessageType::Text(cookies),
            MessageCmd::Replace,
            Some(notify.clone()),
        );
        tx.send(msg).unwrap();
        notify.notified().await;
        update_alist_cookies().await?;
        println!("Cookies is now up to date!");
    }
    Ok(())
}

pub async fn update_alist_cookies() -> Result<String, reqwest::Error> {
    let client = CLIENT.clone();
    let config_lock = CONFIG.read().await;
    let cookie_str = &config_lock.get().cookies;
    let addition_json = serde_json::json!({"cookie": cookie_str});
    let addition = addition_json.to_string();
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, TOKEN.read().await.parse().unwrap());
    let json_data = serde_json::json!({
        "id": 1,
        "mount_path": "/115",
        "driver": "115 Cloud",
        "addition": addition,
    });
    println!("{:?}", json_data);
    client
        .post("http://127.0.0.1:5244/api/admin/storage/update")
        .headers(headers)
        .json(&json_data)
        .send()
        .await?
        .text()
        .await
}

pub async fn get_file_list(path: &str) -> Result<Value, anyhow::Error> {
    let client = CLIENT.clone();
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, TOKEN.read().await.parse().unwrap());
    let json_data = serde_json::json!({
        "path": path,
        "password": "",
        "page": 1,
        "per_page": 0,
        "refresh": false,
    });
    let response = client
        .post("http://127.0.0.1:5244/api/fs/list")
        .headers(headers)
        .json(&json_data)
        .send()
        .await?
        .text()
        .await?;
    let mut response_json: Value = serde_json::from_str(&response)?;
    let file_list = response_json["data"]["content"].take();
    Ok(file_list)
}

pub async fn download_a_task(path: &str, ani_name: &str) -> Result<(), anyhow::Error> {
    let (name, url) = get_file_raw_url(path).await?;
    let mut storge_path = PathBuf::new();
    storge_path.push("downloads/115");
    storge_path.push(&ani_name);
    storge_path.push(&name);
    download_file(&url, &storge_path).await?;
    Ok(())
}

pub async fn check_is_alist_working() -> Result<(), anyhow::Error> {
    let client = CLIENT.clone();
    match client.get("http://127.0.0.1:5244").send().await{
        Ok(response) => {
            if response.status() == 200 {
                Ok(())
            } else {
                Err(anyhow!("Alist is not working! Response: {response:?}"))
            }
        }
        Err(_) => Err(anyhow!("Can not connect to Alist! Is it running?"))
    }
}

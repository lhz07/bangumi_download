use std::{error::Error, sync::RwLock};

use once_cell::sync::Lazy;
use reqwest::header::{AUTHORIZATION, HeaderMap};
use serde_json::Value;

use crate::config_manager::CONFIG;

static CLIENT: Lazy<reqwest::Client> = Lazy::new(|| reqwest::Client::new());
static TOKEN: Lazy<RwLock<String>> = Lazy::new(|| RwLock::new(String::new()));

pub async fn get_alist_token(username: &str, password: &str) -> Result<(), Box<dyn Error>> {
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
    let response_json:Value = serde_json::from_str(&response)?;
    if response_json["code"] == 200{
        let mut token = TOKEN.write()?;
        token.clear();
        token.push_str(response_json["data"]["token"].as_str().unwrap());
    }else {
        return Err("Wrong username or password".into())
    }
    // println!("{}", TOKEN.read()?);
    Ok(())
}

pub async fn update_alist_cookies() -> Result<String, reqwest::Error> {
    let client = CLIENT.clone();
    let config_lock = CONFIG.read().await;
    let cookie_str = config_lock.get_value()["cookies"]
        .as_object()
        .unwrap()
        .iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("; ");
    let addition_json = serde_json::json!({"cookie": cookie_str});
    let addition = addition_json.to_string();
    let token = TOKEN.read().unwrap().clone();
    let mut headers = HeaderMap::new();
    headers.insert(AUTHORIZATION, token.parse().unwrap());
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

use once_cell::sync::Lazy;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{error::Error, path::Path, vec};
use tokio::{
    fs::{self},
    sync::{RwLock, mpsc},
};

use crate::cloud_manager::update_cloud_cookies;

pub static CONFIG: Lazy<RwLock<Config>> = Lazy::new(|| RwLock::new(Config::new()));

#[derive(Debug)]
pub struct Message {
    keys: Vec<String>,
    value: String,
    append: bool,
}

impl Message {
    pub fn new(keys: Vec<String>, value: String, append: bool) -> Self {
        Self {
            keys,
            value,
            append,
        }
    }
}

#[derive(Debug)]
pub struct Config {
    data: Value,
}

impl Config {
    fn new() -> Self {
        let path = Path::new("config.json");
        let old_json = std::fs::read_to_string(path).expect("can not read config.json");
        let data = if path.exists() && !old_json.is_empty() {
            let json = serde_json::from_str(&old_json).unwrap_or_else(|error| {
                eprintln!(
                    "Invalid json format, you may try empty or delete config.json\nError: {error}"
                );
                std::process::exit(1);
            });
            match Self::check_valid(&json) {
                Some(()) => json,
                None => {
                    eprintln!("Some data of config.json is missing!");
                    std::process::exit(1);
                }
            }
        } else {
            // get username and password
            println!("Username:");
            let mut name = String::new();
            std::io::stdin().read_line(&mut name).expect("Failed to read username!");
            name = name.trim().to_string();
            let mut hasher = Sha256::new();
            println!("Password:");
            hasher.update(rpassword::read_password().expect("Failed to read password") + "-https://github.com/alist-org/alist");
            let password = hex::encode(hasher.finalize());
            let cookies = tokio::runtime::Runtime::new().expect("Can not start thread!").block_on(async{
                update_cloud_cookies().await
            });
            let default_config = serde_json::json!({"user":{"name":name, "password": password},"bangumi":{}, "cookies": cookies, "rss_links": {}, "filter": {"611": ["内封"], "583": ["CHT"], "570": ["内封"], "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, "downloading": {}, "temp": {}, "files_to_download": {}});
            let default_json = serde_json::to_string_pretty(&default_config).unwrap();
            std::fs::write(path, default_json).unwrap_or_else(|error| {
                eprintln!("Can not write to path!\nError: {error}");
                std::process::exit(1);
            });
            default_config
        };
        Config { data }
    }

    fn check_valid(json: &Value) -> Option<()> {
        json["user"]["name"].is_string().then_some(())?;
        json["bangumi"].is_object().then_some(())?;
        json["cookies"].is_object().then_some(())?;
        json["downloading"].is_object().then_some(())?;
        json["files_to_download"].is_object().then_some(())?;
        json["filter"]["default"].is_array().then_some(())?;
        json["rss_links"].is_object().then_some(())?;
        json["temp"].is_object().then_some(())?;

        Some(())
    }

    pub fn get_value(&self) -> &Value {
        &self.data
    }

    pub fn get_mut_value(&mut self) -> &mut Value {
        &mut self.data
    }
}

pub fn initial_config() -> Result<(), Box<dyn Error>> {
    futures::executor::block_on(async {
        let _ = CONFIG.read().await;
    });

    Ok(())
}

pub async fn modify_config(mut rx: mpsc::UnboundedReceiver<Message>) -> Result<(), Box<dyn Error>> {
    while let Some(msg) = rx.recv().await {
        let mut new_config = CONFIG.read().await.get_value().clone();
        let mut current = &mut new_config;
        for key in msg.keys.iter() {
            match current.get(key) {
                Some(_) => {
                    current = current.get_mut(key).unwrap();
                    continue;
                }
                None => {
                    current[key] = Value::Object(Default::default());
                    current = current.get_mut(key).unwrap();
                }
            }
        }
        if msg.append {
            if current.is_array() {
                let arr = current.as_array_mut().expect("can not find the vec!");
                arr.push(Value::String(msg.value));
            } else {
                let new_arr = vec![current.clone()];
                *current = Value::Array(new_arr);
            }
        } else {
            *current = Value::String(msg.value);
        }
        *CONFIG.write().await.get_mut_value() = new_config.clone();
        let new_config =
            serde_json::to_string_pretty(&new_config).expect("can not serialize new config");
        fs::write("config.json", new_config)
            .await
            .expect("can not write new config");
    }
    Ok(())
}

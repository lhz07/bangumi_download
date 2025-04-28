use once_cell::sync::Lazy;
use serde_json::Value;
use std::sync::Arc;
use std::path::Path;
use tokio::fs;
use tokio::sync::{Notify, RwLock, mpsc};

use crate::alist_manager::update_alist_cookies;
use crate::{ERROR_STATUS, alist_manager::get_alist_name_passwd, cloud_manager::get_cloud_cookies};

pub static CONFIG: Lazy<RwLock<Config>> = Lazy::new(|| RwLock::new(Config::new()));

#[derive(Debug)]
pub struct Message {
    keys: Vec<String>,
    value: MessageType,
    cmd: MessageCmd,
    notify: Option<Arc<Notify>>,
}

#[derive(Debug)]
pub enum MessageType {
    Text(String),
    List(Vec<String>),
    Map(serde_json::Map<String, Value>),
    None,
}

#[derive(Debug)]
pub enum MessageCmd {
    Append,
    DeleteKey,
    DeleteValue,
    Replace,
}

impl MessageType {
    pub fn to_value(self) -> Value {
        match self {
            Self::Text(text) => Value::String(text),
            Self::List(list) => Value::Array(list.into_iter().map(Value::String).collect()),
            Self::Map(map) => Value::Object(map),
            Self::None => Value::Null,
        }
    }
}

impl Message {
    pub fn new(
        keys: Vec<String>,
        value: MessageType,
        cmd: MessageCmd,
        notify: Option<Arc<Notify>>,
    ) -> Self {
        Self {
            keys,
            value,
            cmd,
            notify,
        }
    }
}

#[derive(Debug)]
pub struct Config {
    data: Value,
}

impl Config {
    fn new() -> Self {
        let data = Value::Null;
        Config { data }
    }

    fn check_valid(json: &Value) -> Option<()> {
        json["user"]["name"].is_string().then_some(())?;
        json["bangumi"].is_object().then_some(())?;
        json["cookies"].is_string().then_some(())?;
        json["downloading_hash"].is_array().then_some(())?;
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

    pub async fn initial_config() -> Result<(), anyhow::Error> {
        let path = Path::new("config.json");
        let mut old_json = String::new();
        let mut sync_cookies = false;
        if path.exists() {
            old_json = std::fs::read_to_string(path).expect("can not read config.json");
        }
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
            let (name, password) = get_alist_name_passwd().await;
            let cookies = get_cloud_cookies().await;
            sync_cookies = true;
            let default_config = serde_json::json!({"user":{"name":name, "password": password},"bangumi":{}, "cookies": cookies, "rss_links": {}, "filter": {"611": ["内封"], "583": ["CHT"], "570": ["内封"], "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, "magnets":{}, "downloading_hash": [], "hash_ani": {}, "temp": {}, "files_to_download": {}});
            let default_json = serde_json::to_string_pretty(&default_config).unwrap();
            std::fs::write(path, default_json).unwrap_or_else(|error| {
                eprintln!("Can not write to path!\nError: {error}");
                std::process::exit(1);
            });
            default_config
        };
        *CONFIG.write().await = Config { data };
        if sync_cookies {
            update_alist_cookies().await.unwrap();
        }
        Ok(())
    }
}

pub async fn modify_config(mut rx: mpsc::UnboundedReceiver<Message>) {
    while let Some(msg) = rx.recv().await {
        let mut new_config = CONFIG.read().await.get_value().clone();
        let mut current = &mut new_config;
        if let MessageCmd::DeleteKey = msg.cmd {
            for key in msg.keys.iter().take(msg.keys.len() - 1) {
                current = current.get_mut(key).unwrap();
            }
        } else {
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
        }
        match msg.cmd {
            MessageCmd::Append => match msg.value {
                MessageType::Map(mut value) => {
                    let map = current.as_object_mut().expect("can not find map!");
                    map.append(&mut value);
                }
                MessageType::List(_) => {
                    if !current.is_array() {
                        *current = Value::Array(Default::default());
                    }
                    let arr = current.as_array_mut().expect("can not find the vec!");
                    arr.append(msg.value.to_value().as_array_mut().unwrap());
                }
                MessageType::Text(_) => {
                    if !current.is_array() {
                        *current = Value::Array(Default::default());
                    }
                    let arr = current.as_array_mut().expect("can not find the vec!");
                    arr.push(msg.value.to_value());
                }
                MessageType::None => (),
            },
            MessageCmd::DeleteKey => {
                current
                    .as_object_mut()
                    .unwrap()
                    .remove(msg.keys.last().unwrap());
            }
            MessageCmd::DeleteValue => {
                let arr = current.as_array_mut().unwrap();
                let del_value = msg.value.to_value();
                if let Some(index) = arr.iter().position(|item| *item == del_value) {
                    arr.remove(index);
                }
            }
            MessageCmd::Replace => {
                *current = msg.value.to_value();
            }
        }
        println!("try to write");
        *CONFIG.write().await.get_mut_value() = new_config.clone();
        println!("wrote!");
        if let Some(notify) = msg.notify {
            notify.notify_one();
            println!("notify the thread");
        }
        #[cfg(not(test))]
        {
            let new_config =
                serde_json::to_string_pretty(&new_config).expect("can not serialize new config");
            fs::write("config.json", new_config)
                .await
                .expect("can not write new config");
        }
    }
    #[cfg(not(test))]
    if *ERROR_STATUS.read().await {
        std::process::exit(1);
    } else {
        std::process::exit(0);
    }
}

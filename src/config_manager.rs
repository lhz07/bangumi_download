use once_cell::sync::Lazy;
use serde_json::Value;
use std::{error::Error, path::Path, vec};
use tokio::{
    fs::{self},
    sync::{RwLock, mpsc},
};

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
        let default_config = serde_json::json!({"bangumi":{}, "cookies": {}, "rss_links": {}, "filter": {"611": ["内封"], "583": ["CHT"], "570": ["内封"], "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, "downloading": {}, "temp": {}, "files_to_download": {}});
        let default_json = serde_json::to_string_pretty(&default_config).unwrap();
        let path = Path::new("config.json");
        let data = if path.exists() {
            match std::fs::read_to_string("config.json").expect("can not read config.json") {
                json_content if !json_content.is_empty() => {
                    serde_json::from_str(&json_content).expect("invalid json!")
                }
                _ => {
                    std::fs::write(path, default_json).expect("can not write to path!");
                    default_config
                }
            }
        } else {
            std::fs::write(path, default_json).expect("can not write to path!");
            default_config
        };
        Config { data }
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

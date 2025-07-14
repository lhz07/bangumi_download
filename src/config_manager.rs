use crate::cloud_manager::get_cloud_cookies;
use arc_swap::ArcSwap;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;
use std::{collections::HashMap, error::Error};
use tokio::sync::{Notify, mpsc};

pub static CONFIG: Lazy<ArcSwap<Config>> = Lazy::new(|| ArcSwap::new(Arc::new(Config::new())));
type MagnetLink = String;
type RSSLink = String;
type BangumiID = String;
type BangumiName = String;
type TimeStamp = String;
type SubgroupID = String;
type Keyword = String;
type Hash = String;
type AniName = String;

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Config {
    pub bangumi: HashMap<BangumiID, TimeStamp>,
    pub cookies: String,
    pub filter: HashMap<SubgroupID, Vec<Keyword>>,
    pub hash_ani: HashMap<Hash, AniName>,
    pub hash_ani_slow: HashMap<Hash, AniName>,
    pub magnets: HashMap<BangumiName, Vec<MagnetLink>>,
    pub rss_links: HashMap<BangumiName, RSSLink>,
}

pub trait Remove<T> {
    fn remove_an_element(&mut self, element: &T);
}

impl<T: PartialEq> Remove<T> for Vec<T> {
    fn remove_an_element(&mut self, element: &T) {
        if let Some(index) = self.iter().position(|item| *item == *element) {
            self.remove(index);
        }
    }
}

// #[derive(Debug)]
pub struct Message {
    pub cmd: Box<dyn FnOnce(&mut Config) -> () + Send + Sync>,
    pub notify: Option<Arc<Notify>>,
}

impl Message {
    pub fn new(
        cmd: Box<dyn FnOnce(&mut Config) -> () + Send + Sync>,
        notify: Option<Arc<Notify>>,
    ) -> Self {
        Self { cmd, notify }
    }
}

impl Config {
    fn new() -> Self {
        let data = Config::default();
        data
    }

    pub async fn initial_config() -> Result<(), Box<dyn Error + Send + Sync>> {
        let path = Path::new("config.json");
        let mut old_json = String::new();
        if path.exists() {
            old_json = std::fs::read_to_string(path).expect("can not read config.json");
        }
        let data = if path.exists() && !old_json.is_empty() {
            serde_json::from_str::<Config>(&old_json).or_else(|error| {
                Err(format!(
                    "Invalid json format, you may try empty or delete config.json\nError: {error}"
                ))
            })?
        } else {
            // get cookies
            let cookies = get_cloud_cookies().await;
            let default_config = serde_json::json!({"bangumi":{}, "cookies": cookies, "rss_links": {}, "filter": {"611": ["内封"], "583": ["CHT"], "570": ["内封"], "default": ["简繁日内封", "简日内封", "简繁内封", "内封", "简体", "简日", "简繁日", "简中", "CHS"]}, "magnets":{}, "hash_ani": {}, "hash_ani_slow": {}, "temp": {}, "files_to_download": {}});
            let default_json = serde_json::to_string_pretty(&default_config)
                .expect("default config should be valid!");
            std::fs::write(path, default_json)
                .or_else(|error| Err(format!("Can not write to path!\nError: {error}")))?;
            serde_json::from_value(default_config).expect("default config should be valid!")
        };
        CONFIG.store(Arc::new(data));
        Ok(())
    }
}

pub async fn modify_config(mut rx: mpsc::UnboundedReceiver<Message>) {
    println!("waiting for the first msg...");
    while let Some(msg) = rx.recv().await {
        let mut new_config = CONFIG.load().as_ref().clone();
        (msg.cmd)(&mut new_config);
        #[cfg(not(test))]
        {
            use tokio::fs;
            let config_str = serde_json::to_string_pretty(&new_config)
                .expect("Config should be a valid struct!");
            fs::write("config.json", config_str)
                .await
                .expect("can not write new config");
        }
        CONFIG.store(Arc::new(new_config));
        if let Some(notify) = msg.notify {
            notify.notify_one();
            println!("notify the thread");
        }
        println!("waiting for the next msg...");
    }
    println!("exit modify config");
}

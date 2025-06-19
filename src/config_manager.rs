use crate::alist_manager::update_alist_cookies;
use crate::{alist_manager::get_alist_name_passwd, cloud_manager::get_cloud_cookies};
use arc_swap::ArcSwap;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
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
    // this will be deprecated
    // files_to_download: HashMap<>
    pub filter: HashMap<SubgroupID, Vec<Keyword>>,
    pub hash_ani: HashMap<Hash, AniName>,
    pub hash_ani_slow: HashMap<Hash, AniName>,
    pub magnets: HashMap<BangumiName, Vec<MagnetLink>>,
    pub rss_links: HashMap<BangumiName, RSSLink>,
    // this will be deprecated
    // temp: HashMap<>
    // this will be deprecated
    pub user: User,
}

// impl Config{
//     pub fn get(&self) -> &Config {
//         self
//     }

//     pub fn get_mut(&mut self) -> &mut Config {
//         self
//     }
// }
// this will be deprecated
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct User {
    pub name: String,
    pub password: String,
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

    pub async fn initial_config() -> Result<(), anyhow::Error> {
        let path = Path::new("config.json");
        let mut old_json = String::new();
        let mut sync_cookies = false;
        if path.exists() {
            old_json = std::fs::read_to_string(path).expect("can not read config.json");
        }
        let data = if path.exists() && !old_json.is_empty() {
            serde_json::from_str::<Config>(&old_json).unwrap_or_else(|error| {
                eprintln!(
                    "Invalid json format, you may try empty or delete config.json\nError: {error}"
                );
                std::process::exit(1);
            })
        } else {
            // get username and password
            let (name, password) = get_alist_name_passwd().await;
            let cookies = get_cloud_cookies().await;
            sync_cookies = true;
            let default_config = serde_json::json!({"user":{"name":name, "password": password},"bangumi":{}, "cookies": cookies, "rss_links": {}, "filter": {"611": ["内封"], "583": ["CHT"], "570": ["内封"], "default": ["简繁日内封", "简日内封", "简繁内封", "内封", "简体", "简日", "简繁日", "简中", "CHS"]}, "magnets":{}, "hash_ani": {}, "hash_ani_slow": {}, "temp": {}, "files_to_download": {}});
            let default_json = serde_json::to_string_pretty(&default_config).unwrap();
            std::fs::write(path, default_json).unwrap_or_else(|error| {
                eprintln!("Can not write to path!\nError: {error}");
                std::process::exit(1);
            });
            serde_json::from_value(default_config)?
        };
        CONFIG.store(Arc::new(data));
        if sync_cookies {
            update_alist_cookies().await.unwrap();
        }
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
            let config_str =
                serde_json::to_string_pretty(&new_config).expect("can not serialize new config");
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
    #[cfg(not(test))]
    {
        use crate::ERROR_STATUS;
        if *ERROR_STATUS.read().await {
            std::process::exit(1);
        } else {
            std::process::exit(0);
        }
    }
}

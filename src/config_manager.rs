use crate::time_stamp::TimeStamp;
use arc_swap::ArcSwap;
use bincode::{Decode, Encode};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::error::Error;
use std::path::Path;
use std::sync::Arc;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::{Notify, mpsc};

pub static CONFIG: Lazy<ArcSwap<Config>> = Lazy::new(|| ArcSwap::new(Arc::new(Config::new())));

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Config {
    /// - `key`: bangumi ID
    /// - `value`: Bangumi
    pub bangumi: HashMap<String, Bangumi>,
    pub cookies: String,
    /// - `key`: subgroup ID
    /// - `value`: SubGroup
    pub filter: HashMap<String, SubGroup>,
    /// - `key`: task hash
    /// - `value`: anime name
    pub hash_ani: HashMap<String, String>,
    /// - `key`: task hash
    /// - `value`: anime name
    pub hash_ani_slow: HashMap<String, String>,
    /// - `key`: bangumi name
    /// - `value`: `Vec<MagnetLink>`
    pub magnets: HashMap<String, Vec<String>>,
    /// - `key`: bangumi ID
    /// - `value`: (bangumi name, rss link)
    pub rss_links: HashMap<String, (String, String)>,
}

#[derive(Debug, Serialize, Deserialize, Default, Clone, Encode, Decode)]
pub struct SubGroup {
    pub name: String,
    pub filter_list: Vec<String>,
}

impl SubGroup {
    pub fn new_const(filter_list: &[&str]) -> Self {
        Self {
            name: String::new(),
            filter_list: filter_list.iter().map(|s| s.to_string()).collect(),
        }
    }
    pub fn new(filter_list: Vec<String>) -> Self {
        Self {
            name: String::new(),
            filter_list,
        }
    }

    pub fn with_name(mut self, name: String) -> Self {
        self.name = name;
        self
    }
}

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct Bangumi {
    pub last_update: TimeStamp,
    pub latest_episode: String,
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

pub trait SafeSend<T> {
    fn send_msg(&self, msg: T);
}

impl SafeSend<Message> for UnboundedSender<Message> {
    fn send_msg(&self, msg: Message) {
        if self.send(msg).is_err() {
            eprintln!("receiver is guaranteed to be dropped after all senders");
        }
    }
}

// #[derive(Debug)]
pub struct Message {
    pub cmd: Box<dyn FnOnce(&mut Config) + Send + Sync>,
    pub notify: Option<Arc<Notify>>,
}

impl Message {
    pub fn new(
        cmd: Box<dyn FnOnce(&mut Config) + Send + Sync>,
        notify: Option<Arc<Notify>>,
    ) -> Self {
        Self { cmd, notify }
    }
}

impl Config {
    fn new() -> Self {
        Config::default()
    }

    pub async fn initial_config() -> Result<(), Box<dyn Error + Send + Sync>> {
        let path = Path::new("config.json");
        let mut old_json = String::new();
        if path.exists() {
            old_json = std::fs::read_to_string(path).expect("can not read config.json");
        }
        let data = if path.exists() && !old_json.is_empty() {
            serde_json::from_str::<Config>(&old_json).map_err(|error| {
                format!(
                    "Invalid json format, you may try to empty or delete config.json\nError: {error}"
                )
            })?
        } else {
            // get cookies
            // let cookies = get_cloud_cookies().await?;
            let mut default_config = Config::default();
            let default_filters = [
                ("611", vec!["内封"]),
                ("583", vec!["CHT"]),
                ("570", vec!["内封"]),
                (
                    "default",
                    vec![
                        "简繁日内封",
                        "简日内封",
                        "简繁内封",
                        "内封",
                        "简体",
                        "简日",
                        "简繁日",
                        "简中",
                        "CHS",
                    ],
                ),
            ];
            let default_filters = default_filters
                .iter()
                .map(|(id, filters)| (id.to_string(), SubGroup::new_const(filters)))
                .collect::<HashMap<String, SubGroup>>();
            default_config.filter = default_filters;
            let default_json = serde_json::to_string_pretty(&default_config)
                .expect("default config should be valid!");
            std::fs::write(path, default_json)
                .map_err(|error| format!("Can not write to path!\nError: {error}"))?;
            default_config
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
            if let Err(e) = fs::write("config.json", config_str).await {
                eprintln!(
                    "CRITICAL ERROR: Can not write new config!\n{}\nExiting...",
                    e
                );
                std::process::exit(1);
            }
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

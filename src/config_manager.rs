use crate::alist_manager::update_alist_cookies;
use crate::{alist_manager::get_alist_name_passwd, cloud_manager::get_cloud_cookies};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
#[cfg(not(test))]
use tokio::fs;
#[cfg(not(test))]
use crate::ERROR_STATUS;
use tokio::sync::{Notify, RwLock, mpsc};

pub static CONFIG: Lazy<RwLock<ConfigManager>> = Lazy::new(|| RwLock::new(ConfigManager::new()));
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
// this will be deprecated
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
pub struct User {
    pub name: String,
    pub password: String,
}

pub trait Replace<T> {
    fn replace_an_element(&mut self, element: T);
}

impl<T> Replace<T> for T {
    fn replace_an_element(&mut self, element: T) {
        *self = element;
    }
}

pub trait Remove<T> {
    fn remove_an_element(&mut self, element: T);
}

impl<T: PartialEq> Remove<T> for Vec<T> {
    fn remove_an_element(&mut self, element: T) {
        if let Some(index) = self.iter().position(|item| *item == element) {
            self.remove(index);
        }
    }
}

// impl<T: Eq + std::hash::Hash, U> Remove<T> for HashMap<T, U> {
//     fn remove_an_element(&mut self, element: T) {
//         self.remove(&element);
//     }
// }

// pub fn remove<T: 'static + PartialEq>(a: &mut dyn Any, key: Option<String>, mut element: T){
//     if a.is::<Vec<T>>() {
//         let content = a.downcast_mut::<Vec<T>>().unwrap();
//         content.remove_an_element(element);
//     }else if a.is::<HashMap<String, String>>(){
//         let content = a.downcast_mut::<HashMap<String, String>>().unwrap();
//         content.remove_an_element(element);
//     }
// }

pub fn remove(a: &mut dyn Any, key: String) {
    if a.is::<HashMap<String, String>>() {
        let content = a.downcast_mut::<HashMap<String, String>>().unwrap();
        content.remove(&key);
    } else if a.is::<HashMap<String, Vec<String>>>() {
        let content = a.downcast_mut::<HashMap<String, Vec<String>>>().unwrap();
        content.remove(&key);
    } else {
        unreachable!("can not remove an element!");
    }
}

pub fn append<T: 'static>(a: &mut dyn Any, key: Option<String>, mut element: T) {
    if a.is::<Vec<T>>() {
        let content = a.downcast_mut::<Vec<T>>().unwrap();
        content.push(element);
    } else if a.is::<HashMap<String, T>>() {
        let content = a.downcast_mut::<HashMap<String, T>>().unwrap();
        content.insert(key.unwrap(), element);
    } else if a.is::<T>() && a.is::<Vec<String>>() {
        let content = a.downcast_mut::<Vec<String>>().unwrap();
        let element = &mut element as &mut dyn Any;
        let list1 = element.downcast_mut::<Vec<String>>().unwrap();
        content.append(list1);
    } else {
        unreachable!("can not append an element!");
    }
}

pub fn replace<'a, T: 'static>(a: &'a mut dyn Any, element: T) {
    let content = a.downcast_mut::<T>().unwrap();
    content.replace_an_element(element);
}

#[derive(Debug)]
pub struct Message {
    pub keys: Vec<String>,
    pub value: MessageType,
    pub cmd: MessageCmd,
    pub notify: Option<Arc<Notify>>,
}

#[derive(Debug)]
pub enum MessageType {
    Text(String),
    List(Vec<String>),
    Map(HashMap<String, String>),
    MapVec(HashMap<String, Vec<String>>),
    None,
}

#[derive(Debug)]
pub enum MessageCmd {
    Append,
    Delete,
    Replace,
}

impl MessageType {
    pub fn replace(self, mut keys: Vec<String>, config: &mut Config) {
        match self {
            MessageType::Text(text) => {
                match config.get_mut_by_path(&keys) {
                    Some(content) => {
                        replace(content, text);
                    }
                    None => {
                        let parent = config.get_mut_by_path(&keys[..keys.len() - 1]).unwrap();
                        let map = parent.downcast_mut::<HashMap<String, String>>().unwrap();
                        map.insert(keys.pop().unwrap(), text);
                    }
                };
            }
            MessageType::List(list) => {
                let content = config.get_mut_by_path(&keys).unwrap();
                replace(content, list);
            }
            MessageType::Map(map) => {
                let content = config.get_mut_by_path(&keys).unwrap();
                replace(content, map);
            }
            MessageType::MapVec(map) => {
                let content = config.get_mut_by_path(&keys).unwrap();
                replace(content, map);
            }
            MessageType::None => panic!("replace can not be used with None"),
        };
    }

    pub fn add(self, keys: Vec<String>, config: &mut Config) {
        fn common_add<T: 'static>(value: T, mut keys: Vec<String>, config: &mut Config) {
            match config.get_mut_by_path(&keys) {
                Some(content) => append(content, None, value),
                None => {
                    let parent = config.get_mut_by_path(&keys[..keys.len() - 1]).unwrap();
                    append(parent, Some(keys.pop().unwrap()), value);
                }
            }
        }
        fn map_add<T: 'static>(
            value: HashMap<String, T>,
            mut keys: Vec<String>,
            config: &mut Config,
        ) {
            match config.get_mut_by_path(&keys) {
                Some(content) if content.is::<HashMap<String, T>>() => {
                    let map1 = content.downcast_mut::<HashMap<String, T>>().unwrap();
                    map1.extend(value.into_iter());
                }
                Some(content) => {
                    append(content, None, value);
                }
                None => {
                    let parent = config.get_mut_by_path(&keys[..keys.len() - 1]).unwrap();
                    append(parent, Some(keys.pop().unwrap()), value);
                }
            }
        }
        match self {
            MessageType::Text(text) => {
                common_add(text, keys, config);
            }
            MessageType::List(list) => {
                common_add(list, keys, config);
            }
            MessageType::Map(map) => {
                map_add(map, keys, config);
            }
            MessageType::MapVec(map) => {
                map_add(map, keys, config);
            }
            MessageType::None => panic!("add can not be used with None"),
        }
    }
    pub fn remove(self, mut keys: Vec<String>, config: &mut Config) {
        fn remove_from_vec<T: 'static + PartialEq>(
            config: &mut Config,
            keys: Vec<String>,
            element: T,
        ) {
            let a = config.get_mut_by_path(&keys).unwrap();
            let content = a.downcast_mut::<Vec<T>>().unwrap();
            content.remove_an_element(element);
        }
        match self {
            MessageType::Text(text) => {
                remove_from_vec(config, keys, text);
            }
            MessageType::List(list) => {
                remove_from_vec(config, keys, list);
            }
            MessageType::Map(map) => {
                remove_from_vec(config, keys, map);
            }
            MessageType::MapVec(map) => {
                remove_from_vec(config, keys, map);
            }
            MessageType::None => {
                let content = config.get_mut_by_path(&keys[..keys.len() - 1]).unwrap();
                remove(content, keys.pop().unwrap());
            }
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
pub struct ConfigManager {
    data: Config,
}

impl ConfigManager {
    fn new() -> Self {
        let data = Config::default();
        ConfigManager { data }
    }

    pub fn get(&self) -> &Config {
        &self.data
    }

    pub fn get_mut(&mut self) -> &mut Config {
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
        *CONFIG.write().await = ConfigManager { data };
        if sync_cookies {
            update_alist_cookies().await.unwrap();
        }
        Ok(())
    }
}

pub async fn modify_config(mut rx: mpsc::UnboundedReceiver<Message>) {
    while let Some(msg) = rx.recv().await {
        let mut new_config = CONFIG.read().await.get().clone();
        match msg.cmd {
            MessageCmd::Replace => msg.value.replace(msg.keys, &mut new_config),
            MessageCmd::Append => msg.value.add(msg.keys, &mut new_config),
            MessageCmd::Delete => msg.value.remove(msg.keys, &mut new_config),
        }
        #[cfg(not(test))]
        {
            let config_str =
                serde_json::to_string_pretty(&new_config).expect("can not serialize new config");
            fs::write("config.json", config_str)
                .await
                .expect("can not write new config");
        }
        // println!("try to write");
        *CONFIG.write().await.get_mut() = new_config;
        // println!("wrote!");
        if let Some(notify) = msg.notify {
            notify.notify_one();
            println!("notify the thread");
        }
    }
    #[cfg(not(test))]
    if *ERROR_STATUS.read().await {
        std::process::exit(1);
    } else {
        std::process::exit(0);
    }
}

pub trait StructPath {
    fn get_mut_by_path(&mut self, path: &[String]) -> Option<&mut dyn Any>;
}

impl StructPath for Config {
    fn get_mut_by_path(&mut self, path: &[String]) -> Option<&mut dyn Any> {
        if path.is_empty() {
            return None;
        }
        match path[0].as_str() {
            "cookies" => {
                if path.len() == 1 {
                    Some(&mut self.cookies as &mut dyn Any)
                } else {
                    None
                }
            }
            "rss_links" => {
                if path.len() == 1 {
                    Some(&mut self.rss_links as &mut dyn Any)
                } else if path.len() > 1 {
                    self.rss_links.get_mut_by_path(&path[1..])
                } else {
                    None
                }
            }
            "user" => self.user.get_mut_by_path(&path[1..]),
            "magnets" => {
                if path.len() == 1 {
                    Some(&mut self.magnets as &mut dyn Any)
                } else if path.len() > 1 {
                    self.magnets.get_mut_by_path(&path[1..])
                } else {
                    None
                }
            }
            "bangumi" => {
                if path.len() == 1 {
                    Some(&mut self.bangumi as &mut dyn Any)
                } else if path.len() > 1 {
                    self.bangumi.get_mut_by_path(&path[1..])
                } else {
                    None
                }
            }
            "filter" => {
                if path.len() == 1 {
                    Some(&mut self.filter as &mut dyn Any)
                } else if path.len() > 1 {
                    self.filter.get_mut_by_path(&path[1..])
                } else {
                    None
                }
            }
            "hash_ani" => {
                if path.len() == 1 {
                    Some(&mut self.hash_ani as &mut dyn Any)
                } else if path.len() > 1 {
                    self.hash_ani.get_mut_by_path(&path[1..])
                } else {
                    None
                }
            }
            "hash_ani_slow" => {
                if path.len() == 1 {
                    Some(&mut self.hash_ani_slow as &mut dyn Any)
                } else if path.len() > 1 {
                    self.hash_ani.get_mut_by_path(&path[1..])
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}

impl<T: Any> StructPath for HashMap<String, T> {
    fn get_mut_by_path(&mut self, path: &[String]) -> Option<&mut dyn Any> {
        self.get_mut(&path[0]).map(|v| v as &mut dyn Any)
    }
}

impl StructPath for User {
    fn get_mut_by_path(&mut self, path: &[String]) -> Option<&mut dyn Any> {
        if path.is_empty() {
            return None;
        }

        match path[0].as_str() {
            "name" => {
                if path.len() == 1 {
                    Some(&mut self.name as &mut dyn Any)
                } else {
                    None
                }
            }
            _ => None,
        }
    }
}
#[macro_export]
macro_rules! match_type {
    ($val:expr, $( $t:ty => $body:block ),+ $(,)?) => {
        {
            let type_id = $val.type_id();
            $(
                if type_id == TypeId::of::<$t>() {
                    $body
                } else
            )*
            {
                println!("Unknown type.");
            }
        }
    };
}

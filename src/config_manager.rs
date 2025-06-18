use anyhow::anyhow;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::path::Path;
use tokio::fs;
use tokio::sync::{Notify, RwLock, mpsc};
use std::any::Any;
use crate::alist_manager::update_alist_cookies;
use crate::{ERROR_STATUS, alist_manager::get_alist_name_passwd, cloud_manager::get_cloud_cookies};

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

#[derive(Debug, Serialize, Deserialize)]
pub struct Config{
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
    pub user: User
}
// this will be deprecated
#[derive(Debug, Serialize, Deserialize)]
pub struct User{
    pub name: String,
    pub password: String,
}

pub trait Add<T>{
    fn add_an_element(&mut self, element: T);
}

impl<T> Add<T> for Vec<T>{
    fn add_an_element(&mut self, element: T) {
        self.push(element);
    }
}

pub trait Replace<T>{
    fn replace_an_element(&mut self, element: T);
}

// impl<String> Replace<String> for String{
//     fn replace_an_element(&mut self, element: String) {
//         *self = element;
//     }
// }
impl<T> Replace<T> for T{
    fn replace_an_element(&mut self, element: T) {
        *self = element;
    }
}
pub fn to_add<'a, T: 'static>(a: &'a mut dyn Any) -> Option<&'a mut (impl Add<T> + use<T>)>{
    if a.is::<Vec<T>>() {
        a.downcast_mut::<Vec<T>>()
    }else{
        None
    }
}

pub fn to_replace<'a, T: 'static>(a: &'a mut dyn Any) -> Option<&'a mut (impl Replace<T> + use<T>)>{
    a.downcast_mut::<T>()
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
                if path.len() == 1{
                    Some(&mut self.rss_links as &mut dyn Any)
                }
                else if path.len() > 1 {
                    self.rss_links.get_mut_by_path(&path[1..])
                }else {
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
            _ => None,
        }
    }
}

impl<T: Any + 'static> StructPath for HashMap<String, T>{
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
pub struct ConfigManager {
    data: Value,
}

impl ConfigManager {
    fn new() -> Self {
        let data = Value::Null;
        ConfigManager { data }
    }

    fn check_valid(json: &Value) -> Result<(), anyhow::Error> {
        json["user"]["name"].is_string().then_some(()).ok_or_else(||anyhow!("missing user: name"))?;
        json["bangumi"].is_object().then_some(()).ok_or_else(||anyhow!("missing bangumi"))?;
        json["cookies"].is_string().then_some(()).ok_or_else(||anyhow!("missing cookies"))?;
        json["hash_ani"].is_object().then_some(()).ok_or_else(||anyhow!("missing hash_ani"))?;
        json["hash_ani_slow"].is_object().then_some(()).ok_or_else(||anyhow!("missing hash_ani"))?;
        json["files_to_download"].is_object().then_some(()).ok_or_else(||anyhow!("missing files_to_download"))?;
        json["filter"]["default"].is_array().then_some(()).ok_or_else(||anyhow!("missing filter: default"))?;
        json["rss_links"].is_object().then_some(()).ok_or_else(||anyhow!("missing rss_links"))?;
        json["temp"].is_object().then_some(()).ok_or_else(||anyhow!("missing temp"))?;
        Ok(())
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
                Ok(()) => json,
                Err(error) => {
                    eprintln!("config.json is {error}");
                    std::process::exit(1);
                }
            }
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
            default_config
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
        // println!("try to write");
        *CONFIG.write().await.get_mut_value() = new_config.clone();
        // println!("wrote!");
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

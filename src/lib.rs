pub mod cli_tools;
pub mod cloud;
pub mod cloud_manager;
pub mod config_manager;
pub mod crypto;
pub mod errors;
pub mod login_with_qrcode;
pub mod main_proc;
pub mod output_tools;
pub mod socket_utils;
pub mod update_rss;

#[cfg(test)]
pub mod tests;

use arc_swap::ArcSwapOption;
use cloud_manager::MOBILE_UA;
use config_manager::Message;
use once_cell::sync::Lazy;
use reqwest::Proxy;
use reqwest_middleware::{ClientBuilder, ClientWithMiddleware};
use reqwest_retry::{RetryTransientMiddleware, policies::ExponentialBackoff};
use std::{sync::atomic::AtomicBool, time::Duration};
use tokio::{
    sync::{Mutex, Notify, Semaphore, mpsc::UnboundedSender},
    task::JoinHandle,
};

use crate::errors::CatError;
pub const PC_UA: &str = "Mozilla/5.0 (X11; Linux x86_64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/134.0.0.0 Safari/537.36";
pub static CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .user_agent(PC_UA)
        .connect_timeout(Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap()
});
pub static CLIENT_DOWNLOAD: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .user_agent(PC_UA)
        .build()
        .unwrap()
});
pub static CLIENT_WITH_RETRY: Lazy<ClientWithMiddleware> = Lazy::new(|| {
    ClientBuilder::new(
        reqwest::Client::builder()
            .user_agent(PC_UA)
            .connect_timeout(Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap(),
    )
    .with(RetryTransientMiddleware::new_with_policy(
        ExponentialBackoff::builder().build_with_max_retries(5),
    ))
    .build()
});
pub static CLIENT_WITH_RETRY_MOBILE: Lazy<ClientWithMiddleware> = Lazy::new(|| {
    ClientBuilder::new(
        reqwest::Client::builder()
            .user_agent(MOBILE_UA)
            .connect_timeout(Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap(),
    )
    .with(RetryTransientMiddleware::new_with_policy(
        ExponentialBackoff::builder().build_with_max_retries(5),
    ))
    .build()
});
pub static CLIENT_PROXY: Lazy<ClientWithMiddleware> = Lazy::new(|| {
    ClientBuilder::new(
        reqwest::Client::builder()
            .user_agent(MOBILE_UA)
            .proxy(Proxy::https("http://127.0.0.1:20171").unwrap())
            .connect_timeout(Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap(),
    )
    .with(RetryTransientMiddleware::new_with_policy(
        ExponentialBackoff::builder().build_with_max_retries(5),
    ))
    .build()
});
pub static TX: Lazy<ArcSwapOption<UnboundedSender<Message>>> =
    Lazy::new(|| ArcSwapOption::new(None));
pub static ERROR_STATUS: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));
pub static REFRESH_DOWNLOAD: Lazy<Mutex<Option<JoinHandle<Result<(), CatError>>>>> =
    Lazy::new(|| Mutex::new(None));
pub static REFRESH_DOWNLOAD_SLOW: Lazy<Mutex<Option<JoinHandle<Result<(), CatError>>>>> =
    Lazy::new(|| Mutex::new(None));
pub static REFRESH_NOTIFY: Lazy<Semaphore> = Lazy::new(|| Semaphore::new(0));
pub static END_NOTIFY: Lazy<Notify> = Lazy::new(|| Notify::new());
pub static EXIT_NOW: Lazy<AtomicBool> = Lazy::new(|| AtomicBool::new(false));

use crate::cloud_manager::{check_cookies, del_cloud_task, download_a_folder, get_tasks_list};
use crate::config_manager::{Bangumi, CONFIG, Config, Message, SafeSend, modify_config};
use crate::errors::{CatError, CloudError, DownloadError};
use crate::id::Id;
use crate::recovery_signal::RECOVERY_SIGNAL;
use crate::socket_utils::{Anime, AsyncReadSocketMsg, AsyncWriteSocketMsg, ClientMsg, ServerMsg};
use crate::update_rss::start_rss_receive;
use crate::{
    BROADCAST_TX, CLIENT_COUNT, END_NOTIFY, LOGIN_STATUS, REFRESH_DOWNLOAD, REFRESH_DOWNLOAD_SLOW,
    REFRESH_NOTIFY, TX,
};
use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::select;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::sync::{Notify, Semaphore};
use tokio::task::JoinHandle;

pub trait ConsumeSema {
    fn consume(&self) -> impl std::future::Future<Output = ()> + Send;
}

impl ConsumeSema for Semaphore {
    async fn consume(&self) -> () {
        self.acquire()
            .await
            .expect("semaphore should be valid")
            .forget();
    }
}

pub struct StatusIter<'a, T> {
    index: usize,
    data: &'a [T],
}

impl<'a, T: Clone> StatusIter<'a, T> {
    pub fn new(data: &'a [T]) -> Self {
        Self { index: 0, data }
    }
    pub fn reset(&mut self) {
        self.index = 0;
    }
    pub fn next_status(&mut self) -> &'a T {
        if self.index < self.data.len() {
            let next_item = &self.data[self.index];
            self.index += 1;
            return next_item;
        } else if self.index == self.data.len() {
            return &self.data[self.index - 1];
        }
        unreachable!("Status iter should always return before this line")
    }
}

pub async fn initialize() -> Result<JoinHandle<()>, Box<dyn std::error::Error + Send + Sync>> {
    // -------------------------------------------------------------------------
    // initial config
    Config::initial_config()
        .await
        .inspect_err(|error| eprintln!("can not initialize config\n{error}"))?;
    // launch config write thread
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
    TX.swap(Some(Arc::new(tx)));
    let config_manager = tokio::spawn(modify_config(rx));
    // -------------------------------------------------------------------------
    check_cookies()
        .await
        .inspect_err(|e| eprintln!("can not check cookies, error: {}", e))?;
    // TODO: handle its error
    let _rss_refresh_handle = tokio::spawn(refresh_rss());
    Ok(config_manager)
}

pub async fn refresh_rss() {
    let waiter = RECOVERY_SIGNAL.get_waiter(crate::recovery_signal::WaiterKind::RefreshRss);
    while !LOGIN_STATUS.load(std::sync::atomic::Ordering::Relaxed) {
        eprintln!("not logged in, waiting...");
        waiter.wait().await;
    }
    // TODO: add actual error handling instead of just printing it
    let download_handle = tokio::spawn(refresh_download());
    let download_slow_handle = tokio::spawn(refresh_download_slow());
    REFRESH_DOWNLOAD.lock().await.replace(download_handle);
    REFRESH_DOWNLOAD_SLOW
        .lock()
        .await
        .replace(download_slow_handle);
    loop {
        println!("\nChecking updates...\n");
        BROADCAST_TX.send_msg(ServerMsg::Loading);
        if let Err(e) = start_rss_receive().await {
            eprintln!("start rss receive error: {}, waiting for recovery", e);
            tokio::select! {
                _ = waiter.wait() => {}
                _ = END_NOTIFY.notified() => {
                    println!("break refresh rss!");
                    break;
                }
            }
            // END_NOTIFY.notify_waiters();
            // break;
        } else {
            println!("\nCheck finished!\n");
            // to avoid cloning data when no client is connected
            if CLIENT_COUNT.load(std::sync::atomic::Ordering::Relaxed) > 0 {
                let config = CONFIG.load();
                let last_updates = &config.bangumi;
                let animes = config
                    .rss_links
                    .iter()
                    .map(|(id, (name, rss_link))| {
                        let latest = match last_updates.get(id) {
                            Some(str) => str.clone(),
                            None => Bangumi::default(),
                        };
                        Anime {
                            id: id.clone(),
                            name: name.clone(),
                            rss_link: rss_link.clone(),
                            last_update: latest.last_update,
                            latest_episode: latest.latest_episode,
                        }
                    })
                    .collect::<Box<_>>();
                BROADCAST_TX.send_msg(ServerMsg::RSSData(animes));
            }
            println!("refresh rss is sleeping");
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(2700)) => {}
                _ = END_NOTIFY.notified() => {
                    println!("break refresh rss!");
                    break;
                }
            }
        }
    }
    println!("exit refresh rss");
}
// TODO: handle error and use this function
// we should check the error immediately instead of checking
// it next time
pub async fn check_refresh_download_error() {
    if let Err(e) = refresh_download().await {
        eprintln!("{e}")
    }
}

pub async fn refresh_download() -> Result<(), CatError> {
    println!("refresh download is started");
    const WAIT_TIME_LIST: [Duration; 6] = [
        Duration::from_secs(10),
        Duration::from_secs(60),
        Duration::from_secs(120),
        Duration::from_secs(120),
        Duration::from_secs(300),
        Duration::from_secs(600),
    ];
    let mut wait_time = StatusIter::new(&WAIT_TIME_LIST);
    let mut error_task = HashMap::new();
    let mut task_download_time: HashMap<String, Instant> = HashMap::new();
    'outer: loop {
        println!("running refresh download");
        let hash_ani = {
            let config = CONFIG.load();
            if config.hash_ani.is_empty() {
                break;
            } else {
                &config.clone().hash_ani
            }
        };
        let tasks_list = match get_tasks_list(hash_ani.keys().collect()).await {
            Ok(list) => list,
            Err(error) => {
                eprintln!("Error occurred when attempting to obtain the task list: {error}");
                println!("Download refresh is stopped!");
                break;
            }
        };
        let tx = TX.load_full().ok_or(CatError::Exit)?;
        for task in tasks_list {
            // download task failed, delete it
            let task_hash = &task.hash;
            if task.status == -1 {
                del_a_task::<HashAni>(&tx, task_hash).await?;
            }
            if task.percent_done == 100 {
                // download file
                let ani_name = hash_ani[task_hash].to_owned();
                println!("Downloading task {}", task.name);
                if let Err(error) = download_a_folder(&task.folder_id, Some(&ani_name)).await {
                    eprintln!("Can not download a task, error: {}", error);
                    error_task
                        .entry(task_hash.to_string())
                        .and_modify(|times| *times += 1)
                        .or_insert(1);
                    if error_task[task_hash] > 2 {
                        break 'outer;
                    }
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    continue;
                }
                // after download
                del_a_task::<HashAni>(&tx, task_hash).await?;
                println!("Task {} is finished and deleted!", task.name);
            } else {
                println!("Task {} is downloading: {}%", task.name, task.percent_done);
                match task_download_time.get(task_hash) {
                    Some(instant) => {
                        if instant.elapsed().as_secs() > 1800 {
                            // move to slow queue
                            let insert_key = task_hash.to_string();
                            let insert_value = hash_ani[task_hash].to_string();
                            let cmd = Box::new(|config: &mut Config| {
                                config.hash_ani_slow.insert(insert_key, insert_value);
                            });
                            let msg = Message::new(cmd, None);
                            tx.send_msg(msg);
                            let notify = Arc::new(Notify::new());
                            let remove_key = task_hash.to_string();
                            let cmd = Box::new(move |config: &mut Config| {
                                config.hash_ani.remove(&remove_key);
                            });
                            let msg = Message::new(cmd, Some(notify.clone()));
                            tx.send_msg(msg);
                            notify.notified().await;
                            task_download_time.remove(task_hash);
                            restart_refresh_download_slow().await?;
                        }
                    }
                    None => {
                        task_download_time.insert(task_hash.to_string(), Instant::now());
                    }
                }
            }
        }
        let wait_task = tokio::time::timeout(*wait_time.next_status(), REFRESH_NOTIFY.consume());
        tokio::select! {
            result = wait_task => {
                match result {
                    Ok(_) => {
                        wait_time.reset();
                        tokio::time::sleep(Duration::from_secs(5)).await;
                        continue;
                    },
                    Err(_) => continue,
                }
            }
            _ = END_NOTIFY.notified() => {
                break;
            }
        }
    }
    println!("refresh download is finished");
    Ok(())
}

pub async fn restart_refresh_download() -> Result<(), CatError> {
    REFRESH_NOTIFY.add_permits(1);
    let handle = REFRESH_DOWNLOAD_SLOW
        .lock()
        .await
        .take_if(|h| h.is_finished());
    if let Some(h) = handle {
        match h.await? {
            Ok(()) => {
                let download_handle = tokio::spawn(refresh_download());
                REFRESH_DOWNLOAD.lock().await.replace(download_handle);
            }
            Err(CatError::Cloud(CloudError::Download(DownloadError::Request(e)))) => {
                eprintln!("{}", e);
                let download_handle = tokio::spawn(refresh_download());
                REFRESH_DOWNLOAD.lock().await.replace(download_handle);
            }
            Err(e) => Err(e)?,
        }
    }
    Ok(())
}

pub async fn restart_refresh_download_slow() -> Result<(), CatError> {
    let handle = REFRESH_DOWNLOAD_SLOW
        .lock()
        .await
        .take_if(|h| h.is_finished());
    if let Some(h) = handle {
        match h.await? {
            Ok(()) => {
                let download_handle = tokio::spawn(refresh_download_slow());
                REFRESH_DOWNLOAD_SLOW.lock().await.replace(download_handle);
            }
            Err(CatError::Cloud(CloudError::Download(DownloadError::Request(e)))) => {
                eprintln!("{}", e);
                let download_handle = tokio::spawn(refresh_download_slow());
                REFRESH_DOWNLOAD_SLOW.lock().await.replace(download_handle);
            }
            Err(e) => Err(e)?,
        }
    }
    Ok(())
}

pub async fn refresh_download_slow() -> Result<(), CatError> {
    println!("refresh download slow is started");
    let wait_time = Duration::from_secs(3600);
    let mut error_task = HashMap::new();
    'outer: loop {
        let hash_ani = {
            let config = CONFIG.load();
            if config.hash_ani.is_empty() {
                break;
            } else {
                &config.clone().hash_ani
            }
        };
        let tasks_list = match get_tasks_list(hash_ani.keys().collect()).await {
            Ok(list) => list,
            Err(error) => {
                eprintln!("Error occurred when attempting to obtain the task list: {error}");
                println!("Download refresh is stopped!");
                break;
            }
        };
        let tx = TX.load_full().ok_or(CatError::Exit)?;
        for task in tasks_list {
            // download task failed, delete it
            let task_hash = &task.hash;
            if task.status == -1 {
                del_a_task::<HashAniSlow>(&tx, task_hash).await?;
            }
            if task.percent_done == 100 {
                // download file
                let ani_name = hash_ani[task_hash].clone();
                println!("Downloading task {}", task.name);
                if let Err(error) = download_a_folder(&task.folder_id, Some(&ani_name)).await {
                    eprintln!("Can not download a task, error: {}", error);
                    error_task
                        .entry(task_hash.to_string())
                        .and_modify(|times| *times += 1)
                        .or_insert(1);
                    if error_task[task_hash] > 3 {
                        break 'outer;
                    }
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    continue;
                }
                // after download
                del_a_task::<HashAniSlow>(&tx, task_hash).await?;
                println!("Task {} is finished and deleted!", task.name);
            }
        }
        tokio::time::sleep(wait_time).await;
    }
    println!("refresh download slow is finished");
    Ok(())
}
struct HashAni;
struct HashAniSlow;
trait DeleteTask {
    fn del_a_task(task_hash: String) -> Box<impl Fn(&mut Config) + Send + Sync>;
}
impl DeleteTask for HashAni {
    fn del_a_task(task_hash: String) -> Box<impl Fn(&mut Config) + Send + Sync> {
        Box::new(move |config: &mut Config| {
            config.hash_ani.remove(&task_hash);
        })
    }
}
impl DeleteTask for HashAniSlow {
    fn del_a_task(task_hash: String) -> Box<impl Fn(&mut Config) + Send + Sync> {
        Box::new(move |config: &mut Config| {
            config.hash_ani_slow.remove(&task_hash);
        })
    }
}
async fn del_a_task<T: DeleteTask + 'static>(
    tx: &UnboundedSender<Message>,
    task_hash: &str,
) -> Result<(), CloudError> {
    del_cloud_task(task_hash).await?;
    let cmd = T::del_a_task(task_hash.to_string());
    let msg = Message::new(cmd, None);
    tx.send_msg(msg);
    Ok(())
}

pub async fn write_socket(
    mut rx: UnboundedReceiver<ServerMsg>,
    mut write: OwnedWriteHalf,
) -> io::Result<()> {
    while let Some(msg) = rx.recv().await {
        write.write_msg(msg).await?;
    }
    Ok(())
}

impl SafeSend<(Id, ClientMsg)> for UnboundedSender<(Id, ClientMsg)> {
    fn send_msg(&self, msg: (Id, ClientMsg)) {
        if let Err(e) = self.send(msg) {
            // log error
            eprintln!(
                "Error occured when sending msg to client msg handler: {}",
                e
            );
        }
    }
}

impl SafeSend<ServerMsg> for UnboundedSender<ServerMsg> {
    fn send_msg(&self, msg: ServerMsg) {
        if let Err(e) = self.send(msg) {
            // log error
            eprintln!(
                "Error occured when sending msg to server msg handler: {}",
                e
            );
        }
    }
}

pub async fn read_socket(
    id: Id,
    tx: UnboundedSender<(Id, ClientMsg)>,
    mut read: OwnedReadHalf,
) -> io::Result<()> {
    loop {
        select! {
            result = read.read_msg() => {
                let msg = result?;
                tx.send_msg((id, msg));
            }
            _ = END_NOTIFY.notified() => {break;}
        }
    }
    Ok(())
}

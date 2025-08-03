use std::{
    collections::HashMap,
    io,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{
    net::unix::{OwnedReadHalf, OwnedWriteHalf},
    select,
    sync::{
        Notify, Semaphore,
        mpsc::{UnboundedReceiver, UnboundedSender},
    },
    task::JoinHandle,
};

use crate::{
    END_NOTIFY, REFRESH_DOWNLOAD, REFRESH_DOWNLOAD_SLOW, REFRESH_NOTIFY, TX,
    cloud_manager::{check_cookies, del_cloud_task, download_a_folder, get_tasks_list},
    config_manager::{CONFIG, Config, Message, SafeSend, modify_config},
    errors::{CatError, CloudError, DownloadError},
    socket_utils::{ReadSocketMsg, SocketMsg, WriteSocketMsg},
    update_rss::start_rss_receive,
};

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
    pub fn next(&mut self) -> &'a T {
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

pub async fn initialize() -> JoinHandle<()> {
    // -------------------------------------------------------------------------
    // initial config
    if let Err(error) = Config::initial_config().await {
        eprintln!("can not initialize config\n{error}");
        std::process::exit(1);
    }
    // launch config write thread
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
    TX.swap(Some(Arc::new(tx)));
    let config_manager = tokio::spawn(modify_config(rx));
    // -------------------------------------------------------------------------
    if let Err(e) = check_cookies().await {
        eprintln!("can not get valid cookies, error: {}", e);
        std::process::exit(1);
    }
    let _rss_refresh_handle = tokio::spawn(refresh_rss());
    // TODO: add actual error handling instead of just printing it
    let download_handle = tokio::spawn(refresh_download());
    let download_slow_handle = tokio::spawn(refresh_download_slow());
    REFRESH_DOWNLOAD.lock().await.replace(download_handle);
    REFRESH_DOWNLOAD_SLOW
        .lock()
        .await
        .replace(download_slow_handle);
    config_manager
}

pub async fn refresh_rss() {
    loop {
        println!("\nChecking updates...\n");
        let rss_links = &CONFIG.load_full().rss_links;
        let urls = rss_links.values().collect::<Vec<&String>>();
        if let Err(e) = start_rss_receive(urls).await {
            eprintln!("start rss receive error: {}", e);
            END_NOTIFY.notify_waiters();
            break;
        }
        println!("\nCheck finished!\n");
        println!("refresh rss is sleeping");
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(2700)) => {}
            _ = END_NOTIFY.notified() => {
                println!("break refresh rss!");
                break;
            }
        }
    }
    println!("exit refresh rss");
}

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
        let wait_task = tokio::time::timeout(*wait_time.next(), REFRESH_NOTIFY.consume());
        tokio::select! {
            result = wait_task => {
                match result {
                    Ok(_) => wait_time.reset(),
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
            Ok(()) => (),
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
            Ok(()) => (),
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
    mut rx: UnboundedReceiver<SocketMsg>,
    mut write: OwnedWriteHalf,
) -> io::Result<()> {
    while let Some(msg) = rx.recv().await {
        write.write_msg(msg).await?;
    }
    // TODO: exit earlier
    println!("write msg to socket exit!");
    Ok(())
}

impl SafeSend<(u128, SocketMsg)> for UnboundedSender<(u128, SocketMsg)> {
    fn send_msg(&self, msg: (u128, SocketMsg)) {
        if let Err(e) = self.send(msg) {
            // log error
            eprintln!(
                "Error occured when sending msg to global msg handler: {}",
                e
            );
        }
    }
}

pub async fn read_socket(
    id: u128,
    tx: UnboundedSender<(u128, SocketMsg)>,
    mut read: OwnedReadHalf,
) -> io::Result<()> {
    loop {
        select! {
            result = read.read_msg() => {
                tx.send_msg((id, result?));
            }
            _ = END_NOTIFY.notified() => {break;}
        }
    }
    Ok(())
}

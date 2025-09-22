use crate::cloud_manager::{download_a_folder, get_cloud_cookies};
use crate::config_manager::{Bangumi, CONFIG, Config, Message, SafeSend, SubGroup};
use crate::errors::{CatError, SocketError};
use crate::id::Id;
use crate::main_proc::{
    read_socket, restart_refresh_download, restart_refresh_download_slow, write_socket,
};
use crate::recovery_signal::{RECOVERY_SIGNAL, Waiting};
use crate::time_stamp::TimeStamp;
use crate::tui::progress_bar::{
    BasicBar, Inc, ProgressBar, ProgressState, ProgressSuit, SimpleBar,
};
use crate::update_rss::{check_rss_link, rss_receive, start_rss_receive};
use crate::{
    BROADCAST_TX, CLIENT_COUNT, CLIENT_WITH_RETRY, END_NOTIFY, LOGIN_STATUS, RSS_DATA_PERMIT, TX,
};
use bincode::{Decode, Encode};
use std::collections::HashMap;
use std::env::temp_dir;
use std::fs;
use std::mem::ManuallyDrop;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::io::{self, AsyncReadExt, AsyncWriteExt};
use tokio::net::unix::SocketAddr;
use tokio::net::{UnixListener, UnixStream};
use tokio::select;
use tokio::sync::Notify;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::task::JoinHandle;
use tokio::time::Instant;

// traits ------------------------------------------------
pub trait SocketStateDetect {
    fn try_connect(&self) -> SocketState;
    fn get_connect_state<P>(&self, path: P) -> SocketState
    where
        P: AsRef<Path>,
    {
        match std::os::unix::net::UnixStream::connect(path) {
            Ok(mut stream) => {
                // to exit this test stream elegantly
                let _ = stream.write_msg(ClientMsg::Exit);
                SocketState::Working
            }
            Err(error) => match error.kind() {
                io::ErrorKind::ConnectionRefused => SocketState::Discarded,
                io::ErrorKind::NotFound => SocketState::NotFound,
                _ => SocketState::Other(error),
            },
        }
    }
}

#[derive(Debug)]
pub enum SocketState {
    Working,
    Discarded,
    NotFound,
    Other(io::Error),
}

// SocketPath ------------------------------------------------
#[derive(Clone)]
pub struct SocketPath {
    pub path: PathBuf,
}

impl SocketPath {
    pub fn new<P>(path: P) -> Self
    where
        P: AsRef<Path>,
    {
        let path = temp_dir().join(path);
        Self { path }
    }

    pub fn to_listener(&self) -> Result<SocketListener, io::Error> {
        SocketListener::bind(&self.path)
    }

    pub fn initial_listener(&self) -> Result<SocketListener, SocketError> {
        let listener = match self.to_listener() {
            Ok(listener) => listener,
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => match self.try_connect() {
                SocketState::Discarded => {
                    println!(
                        "socket path is already in use, and we can not connect it, trying removing it"
                    );
                    std::fs::remove_file(&self.path)?;
                    self.to_listener()?
                }
                SocketState::Other(e) => Err(e)?,
                SocketState::Working => Err("The socket is already working!".to_string())?,
                SocketState::NotFound => Err("Can not find the socket!??".to_string())?,
            },
            Err(e) => Err(e)?,
        };
        Ok(listener)
    }

    pub async fn to_stream(&self) -> Result<UnixStream, io::Error> {
        UnixStream::connect(&self.path).await
    }
}

impl SocketStateDetect for SocketPath {
    fn try_connect(&self) -> SocketState {
        self.get_connect_state(&self.path)
    }
}

#[derive(PartialEq, Eq, Hash)]
pub enum HandleType {
    Login,
    RefreshRSS,
}

// SocketListener ------------------------------------------------
pub struct SocketListener {
    listener: ManuallyDrop<UnixListener>,
    stream_read_tx: UnboundedSender<(Id, ClientMsg)>,
    stream_write_txs: HashMap<Id, UnboundedSender<ServerMsg>>,
    path: PathBuf,
    state: ProgressSuit<ProgressBar>,
    handles: HashMap<HandleType, (Id, JoinHandle<()>)>,
}

impl SocketListener {
    pub fn bind<P>(path: P) -> Result<Self, io::Error>
    where
        P: AsRef<Path>,
    {
        let listener = ManuallyDrop::new(UnixListener::bind(&path)?);
        let (stream_read_tx, _) = unbounded_channel::<(Id, ClientMsg)>();
        Ok(Self {
            listener,
            stream_read_tx,
            stream_write_txs: HashMap::new(),
            path: path.as_ref().to_path_buf(),
            state: ProgressSuit::new(),
            handles: HashMap::new(),
        })
    }
    async fn accept_stream(&mut self, accept_result: io::Result<(UnixStream, SocketAddr)>) {
        match accept_result {
            Ok((stream, _)) => {
                // we need to use another channel, because `OwnedWriteHalf` needs mut to use `write_all`,
                // which means we should use a lock or a channel.
                let (read, write) = stream.into_split();
                let id = Id::generate();
                let (write_tx, write_rx) = unbounded_channel::<ServerMsg>();
                let stream_read_tx = self.stream_read_tx.clone();
                let write_tx_1 = write_tx.clone();
                let count = CLIENT_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                println!("current client count: {}", count + 1);
                tokio::spawn(async move {
                    let _ = read_socket(id, stream_read_tx, read).await;
                    // send a msg to help `write_socket` to close
                    write_tx_1.send_msg(ServerMsg::Exit);
                    println!("read socket task is exited");
                    // we always increase the count before decreasing it
                    debug_assert!(CLIENT_COUNT.load(std::sync::atomic::Ordering::Relaxed) > 0);
                    let count = CLIENT_COUNT.fetch_sub(1, std::sync::atomic::Ordering::Relaxed);
                    println!("current client count: {}", count - 1);
                });
                tokio::spawn(async move {
                    let _ = write_socket(write_rx, write).await;
                    println!("write socket task is exited");
                });
                self.stream_write_txs.insert(id, write_tx);
            }
            Err(err) => eprintln!("accept stream error: {}", err),
        }
    }

    /// broadcast server message to every stream
    fn broadcast(&mut self, msg: ServerMsg) {
        if self.stream_write_txs.is_empty() {
            // explicitly return to avoid collect
            return;
        }
        let mut error_key = Vec::new();
        if let [other @ .., (last_key, last_tx)] =
            self.stream_write_txs.iter().collect::<Vec<_>>().as_slice()
        {
            for (i, tx) in other {
                if let Err(e) = tx.send(msg.clone()) {
                    eprintln!("stream write error: {}", e);
                    error_key.push(**i);
                }
            }
            if let Err(e) = last_tx.send(msg) {
                eprintln!("stream write error: {}", e);
                error_key.push(**last_key);
            }

            for i in error_key {
                self.stream_write_txs.remove(&i);
            }
        }
    }
    /// handle the original server messages, and forward them
    async fn handle_msg_and_broadcast(
        &mut self,
        first_msg: ServerMsg,
        last_send: &mut Instant,
        unhandle_latest: &mut bool,
        interval: &Duration,
        timeout: &Duration,
        deadline: &mut OneShotSleep,
    ) {
        // change the state here
        // if the msg is instant msg, forward it, and if `unhandle_latest` is false, return.
        // if we received an instant msg, and `unhandle_latest` is false, that means the state is in sync,
        // there is no need to send the state again
        if let Some(msg) = self.receive_broadcast(first_msg).await {
            self.broadcast(msg);
            if !*unhandle_latest {
                return;
            }
        }
        if !self.state.is_empty() && last_send.elapsed() >= *interval {
            // send state here
            let state = self.state.state();
            self.broadcast(ServerMsg::DownloadSync(state.into_boxed_slice()));
            *last_send = Instant::now();
            // reset deadline
            deadline.set_instant(*last_send + *timeout);
            *unhandle_latest = false;
        } else {
            *unhandle_latest = true;
        }
    }

    async fn receive_broadcast(&mut self, msg: ServerMsg) -> Option<ServerMsg> {
        // here, we handle the original server messages, and update state with them.

        match msg {
            ServerMsg::Download(ref download_msg) => match download_msg.state {
                DownloadState::Start(ref ptr) => {
                    let (name, size) = ptr.as_ref();
                    let bar = ProgressBar::new(name.to_string(), *size);
                    self.state.add(download_msg.id, bar);
                    Some(msg)
                }
                DownloadState::Downloading(delta) => {
                    if let Some(bar) = self.state.get_bar_mut(download_msg.id) {
                        bar.inc(delta);
                        if bar.is_finished() {
                            println!("remove the bar as it is finished");
                            self.state.remove(download_msg.id);
                            let msg = ServerMsg::Download(DownloadMsg {
                                id: download_msg.id,
                                state: DownloadState::Finished,
                            });
                            return Some(msg);
                        }
                    }
                    None
                }
                DownloadState::Finished => {
                    if self.state.remove(download_msg.id).is_some() {
                        Some(msg)
                    } else {
                        None
                    }
                }
                DownloadState::Failed => {
                    if self.state.remove(download_msg.id).is_some() {
                        Some(msg)
                    } else {
                        None
                    }
                }
            },
            _ => Some(msg),
        }
    }
    /// read the socket msg and handle them
    async fn handle_stream_msg(&mut self, stream_msg: (Id, ClientMsg)) {
        let (msg_id, msg) = stream_msg;
        // state sync message should be send to the exact stream, but other messages
        // should be broadcasted to all streams

        // NOTICE: this is under a tokio::select,
        // so any task that takes a long time should use tokio::spawn
        match msg {
            ClientMsg::SyncQuery => {
                println!("accept sync query!");
                if let Some(tx) = self.stream_write_txs.get(&msg_id) {
                    let progresses = self.state.clone().to_simple_bars();
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
                        .collect::<Vec<_>>();
                    tx.send_msg(ServerMsg::SyncResp(Box::new(SyncInfo {
                        progresses,
                        animes,
                    })));
                    tx.send_msg(ServerMsg::IsLogin(
                        LOGIN_STATUS.load(std::sync::atomic::Ordering::Relaxed),
                    ));
                } else {
                    eprintln!("stream write tx is closed");
                };
            }
            ClientMsg::DownloadFolder(cid) => {
                tokio::spawn(async move {
                    if let Err(e) = download_a_folder(&cid, None).await {
                        eprintln!("download a folder error: {e}");
                        let info = format!(
                            "Can not download the folder: {cid}, please check the cid and login status"
                        );
                        BROADCAST_TX.send_msg(ServerMsg::Error(Box::new((info, e.to_string()))));
                    } else {
                        println!("successfully downloaded a folder");
                        let info = format!("Successfully downloaded the folder: {cid}");
                        BROADCAST_TX.send_msg(ServerMsg::Ok(info.into_boxed_str()));
                    }
                });
            }
            ClientMsg::LoginReq => {
                if self
                    .handles
                    .get(&HandleType::Login)
                    .is_some_and(|(id, h)| *id == msg_id && !h.is_finished())
                {
                    eprintln!("Login handle is already running, ignoring the request");
                } else {
                    let handle = tokio::spawn(async {
                        match get_cloud_cookies().await {
                            Ok(cookies) => match TX.load().as_ref() {
                                Some(tx) => {
                                    let cmd = Box::new(|config: &mut Config| {
                                        config.cookies = cookies;
                                    });
                                    let notify = Arc::new(Notify::new());
                                    let msg = Message::new(cmd, Some(notify.clone()));
                                    tx.send_msg(msg);
                                    notify.notified().await;
                                    BROADCAST_TX
                                        .send_msg(ServerMsg::Ok("Successfully logged in".into()));
                                    LOGIN_STATUS.store(true, std::sync::atomic::Ordering::Relaxed);
                                    RECOVERY_SIGNAL.recover();
                                    BROADCAST_TX.send_msg(ServerMsg::IsLogin(true));
                                }
                                None => {
                                    eprintln!("Can not store cookies, error: {}", CatError::Exit);
                                }
                            },
                            Err(e) => {
                                eprintln!("get cloud cookies error: {e}");
                                BROADCAST_TX.send_msg(ServerMsg::Error(Box::new((
                                    "Failed to get cloud cookies".into(),
                                    e.to_string(),
                                ))));
                                BROADCAST_TX.send_msg(ServerMsg::QrcodeExpired);
                            }
                        }
                    });
                    if let Some(h) = self.handles.insert(HandleType::Login, (msg_id, handle))
                        && !h.1.is_finished()
                    {
                        eprintln!(
                            "a login process of another client is already running, aborting it"
                        );
                        h.1.abort();
                    }
                }
            }
            ClientMsg::DeleteAnime(id) => match TX.load().as_ref() {
                Some(tx) => {
                    let permit = RSS_DATA_PERMIT.acquire().await.unwrap();
                    let cmd = Box::new(move |config: &mut Config| {
                        config.bangumi.remove(&*id);
                        config.rss_links.remove(&*id);
                    });
                    let notify = Arc::new(Notify::new());
                    let msg = Message::new(cmd, Some(notify.clone()));
                    tx.send_msg(msg);
                    notify.notified().await;
                    drop(permit);
                }
                None => {
                    eprintln!("Can not delete anime, error: {}", CatError::Exit);
                }
            },
            ClientMsg::AddRSS(rss_link) => {
                println!("add rss: {}", rss_link);
                let client = CLIENT_WITH_RETRY.clone();
                match check_rss_link(&rss_link, &client).await {
                    Ok(()) => {
                        let rss_update = async || -> Result<(), CatError> {
                            let temp_tx = TX.load();
                            let tx = temp_tx.as_ref().ok_or(CatError::Exit)?;
                            let old_config = CONFIG.load_full();
                            rss_receive(tx, &rss_link, &old_config, &CLIENT_WITH_RETRY).await?;
                            restart_refresh_download().await?;
                            restart_refresh_download_slow().await?;
                            Ok(())
                        };
                        if let Err(e) = rss_update().await {
                            eprintln!("add RSS link error: {e}");
                            BROADCAST_TX.send_msg(ServerMsg::Error(Box::new((
                                "Can not add RSS link".to_string(),
                                e.to_string(),
                            ))));
                        }
                    }
                    Err(e) => {
                        let error = format!("RSS link error: {e}");
                        eprintln!("{}", error);
                        BROADCAST_TX.send_msg(ServerMsg::Error(Box::new((error.clone(), error))));
                    }
                }
            }
            ClientMsg::RefreshRSS => {
                if self
                    .handles
                    .get(&HandleType::RefreshRSS)
                    .is_some_and(|(id, h)| *id == msg_id && !h.is_finished())
                {
                    eprintln!("RSS refresh handle is already running, ignoring the request");
                } else {
                    let refresh = async || -> Result<(), CatError> {
                        println!("\nChecking updates...\n");
                        start_rss_receive().await?;
                        println!("\nCheck finished!\n");
                        Ok(())
                    };
                    let handle = tokio::spawn(async move {
                        match refresh().await {
                            Ok(()) => {
                                println!("successfully refreshed rss");
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
                            Err(e) => {
                                eprintln!("refresh rss error: {e}");
                                BROADCAST_TX.send_msg(ServerMsg::Error(Box::new((
                                    "Can not refresh RSS".to_string(),
                                    e.to_string(),
                                ))));
                            }
                        }
                    });
                    if let Some(h) = self
                        .handles
                        .insert(HandleType::RefreshRSS, (msg_id, handle))
                        && !h.1.is_finished()
                    {
                        eprintln!(
                            "a RSS refresh process requested by another client is already running, aborting it"
                        );
                        h.1.abort();
                    }
                }
            }
            ClientMsg::GetFilters => {
                if let Some(tx) = self.stream_write_txs.get(&msg_id) {
                    let config_guard = CONFIG.load();
                    let filters = config_guard
                        .filter
                        .clone()
                        .into_iter()
                        .map(|(id, subgroup)| Filter { id, subgroup })
                        .collect();
                    tx.send_msg(ServerMsg::SubFilter(filters));
                }
            }
            ClientMsg::InsertFilter(filter) => match TX.load().as_ref() {
                Some(tx) => {
                    let cmd = Box::new(move |config: &mut Config| {
                        match config.filter.get_mut(&filter.id) {
                            Some(s) => {
                                s.filter_list = filter.subgroup.filter_list;
                            }
                            None => {
                                config.filter.insert(filter.id, filter.subgroup);
                            }
                        }
                    });
                    let msg = Message::new(cmd, None);
                    tx.send_msg(msg);
                }
                None => {
                    eprintln!("Can not modify filter rule, error: {}", CatError::Exit);
                }
            },
            ClientMsg::DelFilter(id) => match TX.load().as_ref() {
                Some(tx) => {
                    let cmd = Box::new(move |config: &mut Config| {
                        config.filter.remove(&id);
                    });
                    let msg = Message::new(cmd, None);
                    tx.send_msg(msg);
                }
                None => {
                    eprintln!("Can not modify filter rule, error: {}", CatError::Exit);
                }
            },
            ClientMsg::GetWaitingState => {
                if let Some(tx) = self.stream_write_txs.get(&msg_id) {
                    let state = RECOVERY_SIGNAL.get_waiting_state();
                    tx.send_msg(ServerMsg::WaitingState(state));
                }
            }
            ClientMsg::Recover => {
                RECOVERY_SIGNAL.recover();
            }
            ClientMsg::Exit => {
                println!("Received client exit message");
                self.stream_write_txs.remove(&msg_id);
            }
        }
    }

    pub async fn listening(
        &mut self,
        mut rx: UnboundedReceiver<ServerMsg>,
        stream_read_tx: UnboundedSender<(Id, ClientMsg)>,
        mut stream_read_rx: UnboundedReceiver<(Id, ClientMsg)>,
    ) {
        // replace the default tx
        self.stream_read_tx = stream_read_tx;
        let mut last_send = Instant::now();

        let mut unhandle_latest = false;
        // interval: time interval between sending messages
        let interval = Duration::from_millis(70);
        let timeout = Duration::from_millis(100);
        let mut deadline = OneShotSleep::new();
        loop {
            select! {
                stream_msg = stream_read_rx.recv() => {
                    if let Some(msg) = stream_msg{
                        self.handle_stream_msg(msg).await;
                    }
                }
                accept_result = self.listener.accept() => {
                    self.accept_stream(accept_result).await;
                }
                Some(msg) = rx.recv() => {
                    self.handle_msg_and_broadcast(msg, &mut last_send, &mut unhandle_latest, &interval, &timeout, &mut deadline).await;
                }
                // send the unsend msg
                _ = deadline.wait() => {
                    println!("deadline reached!");
                    if !self.state.is_empty() && unhandle_latest{
                        // send state
                        let state = self.state.state().into_boxed_slice();
                        self.broadcast(ServerMsg::DownloadSync(state));
                        unhandle_latest = false;
                    }
                }
                _ = END_NOTIFY.notified() => {
                    println!("Socket listener is exiting...");
                    break;
                }
            }
        }
    }
}

impl Deref for SocketListener {
    type Target = UnixListener;
    fn deref(&self) -> &Self::Target {
        &self.listener
    }
}

impl SocketStateDetect for SocketListener {
    fn try_connect(&self) -> SocketState {
        self.get_connect_state(&self.path)
    }
}

impl Drop for SocketListener {
    fn drop(&mut self) {
        // we should drop the listener first, then the socket may be discarded
        unsafe { ManuallyDrop::drop(&mut self.listener) };
        let state = self.try_connect();
        if let SocketState::Discarded = state
            && let Err(e) = fs::remove_file(&self.path)
        {
            eprintln!("Can not remove the unix socket file, error: {}", e);
        }
    }
}

pub trait AsyncWriteSocketMsg<T> {
    fn write_msg(&mut self, msg: T) -> impl std::future::Future<Output = io::Result<()>> + Send;
}

pub trait WriteSocketMsg<T> {
    fn write_msg(&mut self, msg: T) -> std::io::Result<()>;
}

pub trait AsyncReadSocketMsg<T> {
    fn read_msg(&mut self) -> impl std::future::Future<Output = Result<T, io::Error>> + Send;
}

impl<T: Encode + std::marker::Send, U: AsyncWriteExt + std::marker::Unpin + std::marker::Send>
    AsyncWriteSocketMsg<T> for U
{
    async fn write_msg(&mut self, msg: T) -> io::Result<()> {
        let config = bincode::config::standard().with_big_endian();
        let content = bincode::encode_to_vec(msg, config).unwrap();
        if content.len() > u32::MAX as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("content length must be less than {} bytes", u32::MAX),
            ));
        }
        self.write_all(&(content.len() as u32).to_be_bytes())
            .await?;
        self.write_all(&content).await?;
        Ok(())
    }
}

impl<T: Encode + std::marker::Send, U: std::io::Write> WriteSocketMsg<T> for U {
    fn write_msg(&mut self, msg: T) -> io::Result<()> {
        let config = bincode::config::standard().with_big_endian();
        let content = bincode::encode_to_vec(msg, config).unwrap();
        if content.len() > u32::MAX as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("content length must be less than {} bytes", u32::MAX),
            ));
        }
        self.write_all(&(content.len() as u32).to_be_bytes())?;
        self.write_all(&content)?;
        Ok(())
    }
}

impl<T: Decode<()> + std::marker::Send, U: AsyncReadExt + std::marker::Unpin + std::marker::Send>
    AsyncReadSocketMsg<T> for U
{
    async fn read_msg(&mut self) -> Result<T, io::Error> {
        let mut len_buf = [0u8; 4];
        if let Err(error) = self.read_exact(&mut len_buf).await {
            match error.kind() {
                io::ErrorKind::UnexpectedEof => {
                    // println!("stream is closed");
                    return Err(error);
                }
                _ => return Err(error),
            }
        }
        let mut content_buf = vec![0u8; u32::from_be_bytes(len_buf) as usize];
        self.read_exact(&mut content_buf).await?;
        let config = bincode::config::standard().with_big_endian();
        let msg = bincode::decode_from_slice::<T, _>(&content_buf, config)
            .unwrap()
            .0;
        Ok(msg)
    }
}

pub struct OneShotSleep {
    instant: Option<Instant>,
}

impl Default for OneShotSleep {
    fn default() -> Self {
        Self::new()
    }
}

impl OneShotSleep {
    pub fn new() -> Self {
        let instant = None;
        OneShotSleep { instant }
    }

    /// set sleep duration
    pub fn set_instant(&mut self, instant: Instant) {
        self.instant = Some(instant);
    }

    /// wait forever or sleep for a duration
    pub async fn wait(&mut self) {
        match self.instant {
            Some(ins) => {
                // this sleep may be interrupted
                tokio::time::sleep_until(ins).await;
                self.instant = None;
            }
            None => {
                // wait forever
                futures::future::pending::<()>().await;
            }
        }
    }
}

#[derive(Encode, Decode, Debug, Clone)]
pub enum ServerMsg {
    Download(DownloadMsg),
    LoginUrl(Box<str>),
    LoginState(Box<str>),
    IsLogin(bool),
    QrcodeExpired,
    Ok(Box<str>),
    Info(Box<str>),
    RSSData(Box<[Anime]>),
    WaitingState(Waiting),
    Loading,
    SubFilter(Box<[Filter]>),
    /// - (error_info, error)
    Error(Box<(String, String)>),
    DownloadSync(Box<[ProgressState]>),
    SyncResp(Box<SyncInfo>),
    Exit,
}

#[derive(Encode, Decode, Debug, Clone)]
pub enum ClientMsg {
    /// - cid
    DownloadFolder(Box<str>),
    LoginReq,
    GetFilters,
    GetWaitingState,
    Recover,
    InsertFilter(Filter),
    /// - id
    DelFilter(String),
    /// - bangumi id
    DeleteAnime(Box<str>),
    /// - RSS link
    AddRSS(Box<str>),
    RefreshRSS,
    SyncQuery,
    Exit,
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct DownloadMsg {
    /// - task id
    pub id: Id,
    pub state: DownloadState,
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct Anime {
    pub id: String,
    pub name: String,
    pub last_update: TimeStamp,
    pub latest_episode: String,
    pub rss_link: String,
}

#[derive(Encode, Decode, Debug, Default, Clone)]
pub struct Filter {
    pub id: String,
    pub subgroup: SubGroup,
}

#[derive(Encode, Decode, Debug, Clone)]
pub enum DownloadState {
    /// - `String`: file name
    /// - `u64`: file size
    Start(Box<(String, u64)>),
    /// - increment size
    Downloading(u64),
    Finished,
    Failed,
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct SyncInfo {
    pub progresses: ProgressSuit<SimpleBar>,
    pub animes: Vec<Anime>,
}

use bincode::{Decode, Encode};
use std::collections::HashMap;
use std::mem::ManuallyDrop;
use std::ops::Deref;
use std::path::Path;
use std::time::Duration;
use std::{env::temp_dir, fs, path::PathBuf};
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::net::unix::SocketAddr;
use tokio::select;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio::time::Instant;
use tokio::{
    io::{self, AsyncReadExt},
    net::UnixListener,
};

use crate::END_NOTIFY;
use crate::cloud_manager::download_a_folder;
use crate::config_manager::SafeSend;
use crate::errors::SocketError;
use crate::id::Id;
use crate::main_proc::{read_socket, write_socket};
use crate::tui::progress_bar::{
    BasicBar, Inc, ProgressBar, ProgressState, ProgressSuit, SimpleBar,
};

// traits ------------------------------------------------
pub trait SocketStateDetect {
    fn try_connect(&self) -> SocketState;
    fn get_connect_state<P>(&self, path: P) -> SocketState
    where
        P: AsRef<Path>,
    {
        match std::os::unix::net::UnixStream::connect(path) {
            Ok(_) => SocketState::Working,
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
                SocketState::Working => Err(format!("The socket is already working!"))?,
                SocketState::NotFound => Err(format!("Can not find the socket!??"))?,
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

// SocketListener ------------------------------------------------
pub struct SocketListener {
    listener: ManuallyDrop<UnixListener>,
    stream_read_tx: UnboundedSender<(Id, SocketMsg)>,
    stream_write_txs: HashMap<Id, UnboundedSender<SocketMsg>>,
    path: PathBuf,
    state: ProgressSuit<ProgressBar>,
}

impl SocketListener {
    pub fn bind<P>(path: P) -> Result<Self, io::Error>
    where
        P: AsRef<Path>,
    {
        let listener = ManuallyDrop::new(UnixListener::bind(&path)?);
        let (stream_read_tx, _) = unbounded_channel::<(Id, SocketMsg)>();
        Ok(Self {
            listener,
            stream_read_tx,
            stream_write_txs: HashMap::new(),
            path: path.as_ref().to_path_buf(),
            state: ProgressSuit::new(),
        })
    }
    async fn accept_stream(&mut self, accept_result: io::Result<(UnixStream, SocketAddr)>) {
        match accept_result {
            Ok((stream, _)) => {
                // we need to use another channel, because `OwnedWriteHalf` needs mut to use `write_all`,
                // which means we should use a lock or a channel.
                let (read, write) = stream.into_split();
                let id = Id::generate();
                let (write_tx, write_rx) = unbounded_channel::<SocketMsg>();
                tokio::spawn(read_socket(id, self.stream_read_tx.clone(), read));
                tokio::spawn(write_socket(write_rx, write));
                self.stream_write_txs.insert(id, write_tx);
            }
            Err(err) => eprintln!("Error: {}", err),
        }
    }
    fn broadcast(&mut self, msg: SocketMsg) {
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

    async fn handle_msg_and_broadcast(
        &mut self,
        first_msg: SocketMsg,
        last_send: &mut Instant,
        unhandle_latest: &mut bool,
        interval: &Duration,
        timeout: &Duration,
        deadline: &mut OneShotSleep,
    ) {
        // change the state here
        // if the msg is instant msg, forward it, and return.
        // a instant msg means that the state is in sync, there is no need
        // to send the state again
        if let Some(msg) = self.receive_broadcast(first_msg).await {
            self.broadcast(msg);
            return;
        }
        if !self.state.is_empty() && last_send.elapsed() >= *interval {
            // send state here
            let state = self.state.state();
            self.broadcast(SocketMsg::DownloadSync(state));
            *last_send = Instant::now();
            // reset deadline
            deadline.set_duration(*timeout);
            *unhandle_latest = false;
        } else {
            *unhandle_latest = true;
        }
    }

    async fn receive_broadcast(&mut self, msg: SocketMsg) -> Option<SocketMsg> {
        // here, we handle the original socket messages, and update state with them.

        match msg {
            SocketMsg::Download(ref download_msg) => match download_msg.state {
                DownloadState::Start((ref name, size)) => {
                    let bar = ProgressBar::new(name.clone(), size);
                    self.state.add(download_msg.id, bar);
                    Some(msg)
                }
                DownloadState::Downloading(delta) => {
                    if let Some(bar) = self.state.get_bar_mut(download_msg.id) {
                        bar.inc(delta);
                        if bar.is_finished() {
                            println!("remove the bar as it is finished");
                            self.state.remove(download_msg.id);
                            let msg = SocketMsg::Download(DownloadMsg {
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
            },
            _ => Some(msg),
        }
    }

    async fn handle_stream_msg(&mut self, stream_msg: (Id, SocketMsg)) {
        let (id, msg) = stream_msg;
        match msg {
            SocketMsg::SyncQuery => {
                println!("accept sync query!");
                if let Some(tx) = self.stream_write_txs.get(&id) {
                    let progresses = self.state.clone().to_simple_bars();
                    tx.send_msg(SocketMsg::SyncResp(SyncInfo { progresses }));
                } else {
                    eprintln!("stream write tx is closed");
                    return;
                };
            }
            SocketMsg::DownloadFolder(cid) => {
                let tx = if let Some(tx) = self.stream_write_txs.get(&id) {
                    tx.clone()
                } else {
                    eprintln!("stream write tx is closed");
                    return;
                };
                tokio::spawn(async move {
                    if let Err(e) = download_a_folder(&cid, None).await {
                        eprintln!("download a folder error: {e}");
                        let info = format!(
                            "Can not download the folder: {cid}, please check the cid and login status"
                        );
                        tx.send_msg(SocketMsg::Error((info, e.to_string())));
                    } else {
                        println!("successfully downloaded a folder");
                        let info = format!("Successfully downloaded the folder: {cid}");
                        tx.send_msg(SocketMsg::Ok(info));
                    }
                });
            }
            // no need to handle these messages
            SocketMsg::DownloadSync(_) => (),
            SocketMsg::Download(_) => (),
            SocketMsg::SyncResp(_) => (),
            SocketMsg::Null => (),
            SocketMsg::Ok(_) => (),
            SocketMsg::Error(_) => (),
            // async fn handle_msg(msg: SocketMsg, tx: &UnboundedSender<SocketMsg>) {
            // ADD_LINK => {
            // let rss_link = stream.read_str().await?;
            // let client = CLIENT_WITH_RETRY.clone();
            // match check_rss_link(&rss_link, client).await {
            //     Ok(()) => {
            // let temp_tx = TX.load();
            // let tx = temp_tx.as_ref().ok_or(CatError::Exit)?;
            // let old_config = CONFIG.load_full();
            // let rss_update = async || -> Result<(), CatError> {
            //     rss_receive(tx, &rss_link, &old_config, CLIENT_WITH_RETRY.clone())
            // .await?;
            //     restart_refresh_download().await?;
            //     restart_refresh_download_slow().await?;
            //     Ok(())
            // };
            // if let Err(e) = rss_update().await {
            //     eprintln!("add link error: {e}");
            // }
            //     }
            //     Err(error) => stream.write_str(&error).await?,
            // }
            //     }
            //     DOWNLOAD_FOLDER => {
            // let cid = stream.read_str().await?;
            // if let Err(e) = download_a_folder(&cid, None).await {
            //     stream.write_str(&e.to_string()).await?;
            // }
            //     }
            // }
        }
    }

    pub async fn listening(
        &mut self,
        mut rx: UnboundedReceiver<SocketMsg>,
        stream_read_tx: UnboundedSender<(Id, SocketMsg)>,
        mut stream_read_rx: UnboundedReceiver<(Id, SocketMsg)>,
    ) {
        // replace the default tx
        self.stream_read_tx = stream_read_tx;
        let mut last_send = Instant::now();

        let mut unhandle_latest = false;
        // internal: time internal between sending messages
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
                        let state = self.state.state();
                        self.broadcast(SocketMsg::DownloadSync(state));
                        unhandle_latest = false;
                    }
                }
                _ = END_NOTIFY.notified() => {}
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
        if let SocketState::Discarded = state {
            if let Err(e) = fs::remove_file(&self.path) {
                eprintln!("Can not remove the unix socket file, error: {}", e);
            }
        }
    }
}

pub trait WriteSocketMsg {
    fn write_msg(
        &mut self,
        msg: SocketMsg,
    ) -> impl std::future::Future<Output = io::Result<()>> + Send;
}

pub trait ReadSocketMsg {
    fn read_msg(
        &mut self,
    ) -> impl std::future::Future<Output = Result<SocketMsg, io::Error>> + Send;
}

impl<T: AsyncWriteExt + std::marker::Unpin + std::marker::Send> WriteSocketMsg for T {
    async fn write_msg(&mut self, msg: SocketMsg) -> io::Result<()> {
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

impl<T: AsyncReadExt + std::marker::Unpin + std::marker::Send> ReadSocketMsg for T {
    async fn read_msg(&mut self) -> Result<SocketMsg, io::Error> {
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
        let msg = bincode::decode_from_slice::<SocketMsg, _>(&content_buf, config)
            .unwrap()
            .0;
        Ok(msg)
    }
}

// pub struct OneShotSleep1 {
//     notify: Notify,
//     duration: Mutex<Option<Duration>>,
// }

// impl OneShotSleep1 {
//     pub fn new() -> Self {
//         let notify = Notify::new();
//         let duration = Mutex::new(None);
//         Self { notify, duration }
//     }

//     /// set sleep duration
//     pub fn set_duration(&self, dur: Duration) {
//         let mut guard = self.duration.lock().unwrap();
//         *guard = Some(dur);
//         drop(guard);
//         self.notify.notify_waiters();
//     }

//     /// wait for a wake
//     pub async fn wait(&self) {
//         let mut guard = self.duration.lock().unwrap();
//         if let Some(dur) = guard.take() {
//             drop(guard);
//             tokio::time::sleep(dur).await;
//             return;
//         }
//         drop(guard);
//         // wait for new sleep duration
//         self.notify.notified().await;
//     }
// }

pub struct OneShotSleep {
    duration: Option<Duration>,
}

impl OneShotSleep {
    pub fn new() -> Self {
        let duration = None;
        OneShotSleep { duration }
    }

    /// set sleep duration
    pub fn set_duration(&mut self, dur: Duration) {
        self.duration = Some(dur);
    }

    /// wait forever or sleep for a duration
    pub async fn wait(&mut self) {
        if let Some(dur) = self.duration.take() {
            tokio::time::sleep(dur).await;
            return;
        }
        // wait forever
        futures::future::pending::<()>().await;
    }
}

#[derive(Encode, Decode, Debug, Clone)]
pub enum SocketMsg {
    Download(DownloadMsg),
    /// - cid
    DownloadFolder(String),
    Ok(String),
    Error((String, String)),
    DownloadSync(Vec<ProgressState>),
    SyncQuery,
    SyncResp(SyncInfo),
    Null,
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct DownloadMsg {
    /// - task id
    pub id: Id,
    pub state: DownloadState,
}

#[derive(Encode, Decode, Debug, Clone)]
pub enum DownloadState {
    /// - `String`: file name
    /// - `u64`: file size
    Start((String, u64)),
    /// - increment size
    Downloading(u64),
    Finished,
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct SyncInfo {
    pub progresses: ProgressSuit<SimpleBar>,
}

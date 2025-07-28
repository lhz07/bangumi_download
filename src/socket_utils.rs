use bincode::{Decode, Encode};
use futures::future::join3;
use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::{env::temp_dir, fs, path::PathBuf};
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::select;
use tokio::sync::broadcast;
use tokio::sync::mpsc::unbounded_channel;
use tokio::{
    io::{self, AsyncReadExt},
    net::UnixListener,
};

use crate::errors::{CatError, SocketError};
use crate::tui::progress_bar::SimpleBar;
use crate::{END_NOTIFY, main_proc};

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

    pub async fn to_stream(&self) -> Result<SocketStream, io::Error> {
        SocketStream::connect_stream(&self.path).await
    }
}

impl SocketStateDetect for SocketPath {
    fn try_connect(&self) -> SocketState {
        self.get_connect_state(&self.path)
    }
}

// SocketStream ------------------------------------------------
pub struct SocketStream {
    stream: UnixStream,
}

impl SocketStream {
    pub async fn connect_stream<P>(path: P) -> Result<Self, io::Error>
    where
        P: AsRef<Path>,
    {
        let stream = UnixStream::connect(path).await?;
        Ok(Self { stream })
    }

    pub fn new(stream: UnixStream) -> Self {
        Self { stream }
    }

    pub fn split(self) -> (OwnedReadHalf, OwnedWriteHalf) {
        self.stream.into_split()
    }

    pub async fn write_str(&mut self, content: &str) -> Result<(), io::Error> {
        if content.bytes().len() > u32::MAX as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("content length must be less than {} bytes", u32::MAX),
            ));
        }
        self.stream
            .write_all(&(content.bytes().len() as u32).to_be_bytes())
            .await?;
        self.stream.write_all(content.as_bytes()).await
    }

    pub async fn read_str(&mut self) -> Result<String, io::Error> {
        let mut len_buf = [0u8; 4];
        if let Err(error) = self.stream.read_exact(&mut len_buf).await {
            match error.kind() {
                io::ErrorKind::UnexpectedEof => return Ok(String::new()),
                _ => return Err(error),
            }
        }
        let mut content_buf = vec![0u8; u32::from_be_bytes(len_buf) as usize];
        self.stream.read_exact(&mut content_buf).await?;
        Ok(String::from_utf8_lossy(&content_buf).to_string())
    }

    pub async fn read_msg(&mut self) -> Result<SocketMsg, io::Error> {
        let mut len_buf = [0u8; 4];
        if let Err(error) = self.stream.read_exact(&mut len_buf).await {
            match error.kind() {
                io::ErrorKind::UnexpectedEof => return Ok(SocketMsg::Null),
                _ => return Err(error),
            }
        }
        let mut content_buf = vec![0u8; u32::from_be_bytes(len_buf) as usize];
        self.stream.read_exact(&mut content_buf).await?;
        let config = bincode::config::standard().with_big_endian();
        let msg = bincode::decode_from_slice::<SocketMsg, _>(&content_buf, config)
            .unwrap()
            .0;
        Ok(msg)
    }

    pub async fn write_msg(&mut self, msg: SocketMsg) -> io::Result<()> {
        let config = bincode::config::standard().with_big_endian();
        let content = bincode::encode_to_vec(msg, config).unwrap();
        if content.len() > u32::MAX as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("content length must be less than {} bytes", u32::MAX),
            ));
        }
        self.stream
            .write_all(&(content.len() as u32).to_be_bytes())
            .await?;
        self.stream.write_all(&content).await?;
        Ok(())
    }

    pub async fn read_str_to_end(&mut self) -> Result<(), io::Error> {
        loop {
            let response = self.read_str().await?;
            if response == "\0" {
                return Ok(());
            }
            println!("{}", response);
        }
    }

    async fn handle_stream(
        stream: SocketStream,
        state: SyncInfo,
        rx: broadcast::Receiver<SocketMsg>,
    ) -> Result<(), CatError> {
        // here we should pass the SocketMsg received from rx transparently
        // we need to use another channel, because `OwnedWriteHalf` needs mut to use `write_all`,
        // which means we should use a lock or a channel.
        let (read, write) = stream.split();

        let (socket_tx, socket_rx) = unbounded_channel::<SocketMsg>();
        let (read_result, write_result, _) = join3(
            main_proc::read_socket(socket_tx.clone(), state, read),
            main_proc::write_socket(socket_rx, write),
            main_proc::forward_socket_msg(socket_tx, rx),
        )
        .await;
        read_result?;
        write_result?;
        Ok(())
    }
}

impl Deref for SocketStream {
    type Target = UnixStream;
    fn deref(&self) -> &Self::Target {
        &self.stream
    }
}

impl DerefMut for SocketStream {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.stream
    }
}

// SocketListener ------------------------------------------------
pub struct SocketListener {
    listener: ManuallyDrop<UnixListener>,
    path: PathBuf,
    state: SyncInfo,
}

impl SocketListener {
    pub fn bind<P>(path: P) -> Result<Self, io::Error>
    where
        P: AsRef<Path>,
    {
        let listener = ManuallyDrop::new(UnixListener::bind(&path)?);
        let state = SyncInfo {
            progresses: Vec::new(),
        };
        Ok(Self {
            listener,
            path: path.as_ref().to_path_buf(),
            state,
        })
    }
    async fn accept_stream(&self, tx: &broadcast::Sender<SocketMsg>) {
        match self.listener.accept().await {
            Ok((stream, _)) => {
                let rx = tx.subscribe();
                let state = self.state.clone();
                let stream = SocketStream::new(stream);

                tokio::spawn(SocketStream::handle_stream(stream, state, rx));
            }
            Err(err) => eprintln!("Error: {}", err),
        }
    }

    async fn receive_broadcast(
        &mut self,
        recv_result: Result<SocketMsg, broadcast::error::RecvError>,
    ) {
        // here, we handle the original socket messages, and update state with them.
        if let Ok(msg) = recv_result {
            match msg {
                SocketMsg::Download(msg) => match msg.state {
                    DownloadState::Start((name, size)) => {
                        let simple_bar = SimpleBar::new(name, msg.id, size);
                        self.state.progresses.push(simple_bar);
                    }
                    DownloadState::Downloading(delta) => {
                        for progress in &mut self.state.progresses {
                            if progress.id() == msg.id {
                                progress.inc(delta);
                            }
                        }
                    }
                    DownloadState::Finished => {
                        self.state
                            .progresses
                            .retain(|progress| progress.id() != msg.id);
                    }
                },
                _ => (),
            }
        }
    }

    pub async fn listening(
        &mut self,
        tx: broadcast::Sender<SocketMsg>,
        mut rx: broadcast::Receiver<SocketMsg>,
    ) {
        loop {
            select! {
                _ = self.accept_stream(&tx) => {}
                recv_result = rx.recv() => {
                    self.receive_broadcast(recv_result).await;
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

impl WriteSocketMsg for OwnedWriteHalf {
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

impl ReadSocketMsg for OwnedReadHalf {
    async fn read_msg(&mut self) -> Result<SocketMsg, io::Error> {
        let mut len_buf = [0u8; 4];
        if let Err(error) = self.read_exact(&mut len_buf).await {
            match error.kind() {
                io::ErrorKind::UnexpectedEof => return Ok(SocketMsg::Null),
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

#[derive(Encode, Decode, Debug, Clone)]
pub enum SocketMsg {
    Download(DownloadMsg),
    SyncQuery,
    SyncResp(SyncInfo),
    Null,
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct DownloadMsg {
    /// task id
    pub id: String,
    pub state: DownloadState,
}

#[derive(Encode, Decode, Debug, Clone)]
pub enum DownloadState {
    /// (name, size)
    Start((String, u64)),
    /// increments
    Downloading(u64),
    Finished,
}

#[derive(Encode, Decode, Debug, Clone)]
pub struct SyncInfo {
    progresses: Vec<SimpleBar>,
}

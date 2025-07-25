use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut};
use std::path::Path;
use std::{env::temp_dir, fs, path::PathBuf};

use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::{
    io::{self, AsyncReadExt},
    net::UnixListener,
};

use crate::cli_tools::{ADD_LINK, DOWNLOAD_FOLDER};
use crate::cloud_manager::download_a_folder;
use crate::errors::{CatError, SocketError};
use crate::main_proc::{restart_refresh_download, restart_refresh_download_slow};

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

pub trait SocketStreamHandle {
    fn handle_stream(
        stream: SocketStream,
    ) -> impl std::future::Future<Output = Result<(), CatError>> + Send;
}

#[derive(Debug)]
pub enum SocketState {
    Working,
    Discarded,
    NotFound,
    Other(io::Error),
}

// SocketPath ------------------------------------------------
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

    pub async fn read_str_to_end(&mut self) -> Result<(), io::Error> {
        loop {
            let response = self.read_str().await?;
            if response == "\0" {
                return Ok(());
            }
            println!("{}", response);
        }
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
}

impl SocketListener {
    pub fn bind<P>(path: P) -> Result<Self, io::Error>
    where
        P: AsRef<Path>,
    {
        let listener = ManuallyDrop::new(UnixListener::bind(&path)?);
        Ok(Self {
            listener,
            path: path.as_ref().to_path_buf(),
        })
    }

    pub async fn listening(&self) {
        loop {
            match self.listener.accept().await {
                Ok((stream, _)) => {
                    let stream = SocketStream::new(stream);
                    tokio::spawn(SocketStream::handle_stream(stream));
                }
                Err(err) => eprintln!("Error: {}", err),
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

impl SocketStreamHandle for SocketStream {
    async fn handle_stream(mut stream: SocketStream) -> Result<(), CatError> {
        use crate::{
            CLIENT_WITH_RETRY, TX,
            config_manager::CONFIG,
            update_rss::{check_rss_link, rss_receive},
        };

        let keep_alive = stream.read_str().await? == "keep-alive";
        loop {
            let content = stream.read_str().await?;
            match content.as_str() {
                // implement add link
                ADD_LINK => {
                    let rss_link = stream.read_str().await?;
                    let client = CLIENT_WITH_RETRY.clone();
                    match check_rss_link(&rss_link, client).await {
                        Ok(()) => {
                            let temp_tx = TX.load();
                            let tx = temp_tx.as_ref().ok_or(CatError::Exit)?;
                            let old_config = CONFIG.load_full();
                            let rss_update = async || -> Result<(), CatError> {
                                rss_receive(tx, &rss_link, &old_config, CLIENT_WITH_RETRY.clone())
                                    .await?;
                                restart_refresh_download().await?;
                                restart_refresh_download_slow().await?;
                                Ok(())
                            };
                            if let Err(e) = rss_update().await {
                                eprintln!("add link error: {e}");
                            }
                        }
                        Err(error) => stream.write_str(&error).await?,
                    }
                    stream.write_str("\0").await?;
                }
                DOWNLOAD_FOLDER => {
                    let cid = stream.read_str().await?;
                    if let Err(e) = download_a_folder(&cid, None).await {
                        stream.write_str(&e.to_string()).await?;
                    }
                    stream.write_str("\0").await?;
                }
                "" => (),
                _ => stream.write_str("response from server").await?,
            }
            if !keep_alive {
                break;
            }
        }
        Ok(())
    }
}

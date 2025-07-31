use base64::DecodeError;
use reqwest::header::InvalidHeaderValue;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CatError {
    #[error("Socket error: {0}")]
    Socket(#[from] SocketError),
    #[error("RSS parse error: {0}")]
    RSS(#[from] quick_xml::de::DeError),
    #[error("Thread Join error: {0}")]
    Join(#[from] tokio::task::JoinError),
    #[error("Bangumi parse error: {0}")]
    Parse(String),
    #[error(transparent)]
    Cloud(#[from] CloudError),
    #[error("Get cookie error: {0}")]
    GetCookie(String),
    #[error("Deserialize error: {0}")]
    Deserialize(#[from] serde_json::Error),
    #[error("Exiting now...")]
    Exit,
}

#[derive(Error, Debug)]
pub enum CloudError {
    #[error("Decrypt error: {0}")]
    Decrypt(#[from] DecodeError),
    #[error("Cookies error: {0}")]
    CookiesParse(#[from] InvalidHeaderValue),
    #[error("Cookies parse error: {0}")]
    Cookies(String),
    #[error("Deserialize error: {0}")]
    Deserialize(#[from] serde_json::Error),
    #[error("Api error: {0}")]
    Api(String),
    #[error(transparent)]
    Download(#[from] DownloadError),
    #[error("Param error: {0}")]
    Param(String),
    #[error("Download errors: {0:?}")]
    DownloadErrors(Vec<DownloadError>),
}

#[derive(Error, Debug)]
pub enum DownloadError {
    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),
    #[error(transparent)]
    Request(#[from] RequestError),
    #[error("Path error: {0}")]
    Path(String),
    #[error("Content Length error: {0}")]
    ContentLength(String),
}

#[derive(Error, Debug)]
pub enum SocketError {
    #[error("IO error: {0}")]
    IO(#[from] std::io::Error),
    #[error("Other error: {0}")]
    Other(String),
}

impl From<String> for SocketError {
    fn from(value: String) -> Self {
        Self::Other(value)
    }
}

impl From<String> for CloudError {
    fn from(value: String) -> Self {
        Self::Api(value)
    }
}

impl From<reqwest::Error> for DownloadError {
    fn from(value: reqwest::Error) -> Self {
        Self::Request(RequestError::Client(value))
    }
}

impl From<reqwest_middleware::Error> for DownloadError {
    fn from(value: reqwest_middleware::Error) -> Self {
        Self::Request(RequestError::Middleware(value))
    }
}

impl From<reqwest::Error> for CloudError {
    fn from(value: reqwest::Error) -> Self {
        Self::Download(DownloadError::Request(RequestError::Client(value)))
    }
}

impl From<reqwest_middleware::Error> for CloudError {
    fn from(value: reqwest_middleware::Error) -> Self {
        Self::Download(DownloadError::Request(RequestError::Middleware(value)))
    }
}

impl From<reqwest::Error> for CatError {
    fn from(value: reqwest::Error) -> Self {
        Self::Cloud(CloudError::Download(DownloadError::Request(
            RequestError::Client(value),
        )))
    }
}

impl From<reqwest_middleware::Error> for CatError {
    fn from(value: reqwest_middleware::Error) -> Self {
        Self::Cloud(CloudError::Download(DownloadError::Request(
            RequestError::Middleware(value),
        )))
    }
}

impl From<DownloadError> for CatError {
    fn from(value: DownloadError) -> Self {
        Self::Cloud(CloudError::Download(value))
    }
}

impl From<std::io::Error> for CatError {
    fn from(value: std::io::Error) -> Self {
        Self::Socket(SocketError::IO(value))
    }
}

#[derive(Error, Debug)]
pub enum RequestError {
    #[error("HTTP client error: {0}")]
    Client(#[from] reqwest::Error),
    #[error("Request middleware error: {0}")]
    Middleware(#[from] reqwest_middleware::Error),
    #[error("Status error: {0}")]
    Status(String),
}

impl From<String> for RequestError {
    fn from(value: String) -> Self {
        Self::Status(value)
    }
}

impl From<String> for DownloadError {
    fn from(value: String) -> Self {
        Self::Request(RequestError::Status(value))
    }
}

use base64::DecodeError;
use reqwest::header::InvalidHeaderValue;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CatError {
    #[error(transparent)]
    Download(#[from] DownloadError),
    #[error(transparent)]
    Cloud(#[from] CloudError),
    #[error("Get cookie error: {0}")]
    GetCookie(String),
    #[error("Deserialize error: {0}")]
    Deserialize(#[from] serde_json::Error),
    #[error("Exiting: {0}")]
    Exit(String),
}

#[derive(Error, Debug)]
pub enum CloudError {
    #[error("Decrypt error: {0}")]
    Decrypt(#[from] DecodeError),
    #[error("Cookies error: {0}")]
    Cookies(#[from] InvalidHeaderValue),
    #[error("Cookies parse error: {0}")]
    CookiesParse(String),
    #[error("Deserialize error: {0}")]
    Deserialize(#[from] serde_json::Error),
    #[error("Api error: {0}")]
    Api(String),
    #[error(transparent)]
    Download(#[from] DownloadError),
    #[error("Param error: {0}")]
    Param(String),
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

#[derive(Error, Debug)]
pub enum RequestError {
    #[error("HTTP client error: {0}")]
    Client(#[from] reqwest::Error),

    #[error("Request middleware error: {0}")]
    Middleware(#[from] reqwest_middleware::Error),
}

use reqwest::header::InvalidHeaderValue;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CatError {
    #[error(transparent)]
    Request(#[from] RequestError),
    #[error("Api error: {0}")]
    Api(String),
    #[error("Cookies error: {0}")]
    Cookies(#[from] InvalidHeaderValue),
}

impl From<String> for CatError {
    fn from(value: String) -> Self {
        Self::Api(value)
    }
}

impl From<reqwest::Error> for CatError {
    fn from(value: reqwest::Error) -> Self {
        Self::Request(RequestError::Client(value))
    }
}

impl From<reqwest_middleware::Error> for CatError {
    fn from(value: reqwest_middleware::Error) -> Self {
        Self::Request(RequestError::Middleware(value))
    }
}

#[derive(Error, Debug)]
pub enum RequestError {
    #[error("HTTP client error: {0}")]
    Client(#[from] reqwest::Error),

    #[error("Request middleware error: {0}")]
    Middleware(#[from] reqwest_middleware::Error),
}

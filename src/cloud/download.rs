use std::{collections::HashMap, time::UNIX_EPOCH};

use crate::{
    config_manager::CONFIG,
    crypto::{rsa, xor},
    errors::CloudError,
};
use base64::{DecodeError, Engine, engine::general_purpose};
use rand::{self, Rng};
use reqwest::header::{CONTENT_TYPE, COOKIE, HeaderMap};
use reqwest_middleware::ClientWithMiddleware;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct DownloadResponse {
    pub state: bool,
    pub msg: String,
    pub data: String,
}

#[derive(Debug, Deserialize)]
pub struct FileDownloadUrl {
    pub client: f64,
    pub oss_id: String,
    pub url: String,
}

#[derive(Debug, Deserialize)]
pub struct DownloadInfo {
    pub file_name: String,
    pub file_size: String,
    pub url: FileDownloadUrl,
}

type DownloadData = HashMap<String, DownloadInfo>;

pub fn encode(mut input: Vec<u8>, key: &[u8]) -> String {
    // Prepare buffer
    let mut buf = key.to_vec();
    // Copy key and data to buffer
    buf.append(&mut input);
    // XOR encode
    xor::xor_transform(&mut buf[16..], &xor::xor_derive_key(&key, 4));
    buf[16..].reverse();
    xor::xor_transform(&mut buf[16..], &xor::XOR_CLIENT_KEY);
    // Encrypt and encode
    let encrypt = rsa::rsa_encrypt(&buf);
    general_purpose::STANDARD.encode(&encrypt)
}

pub fn decode(input: String, key: &[u8]) -> Result<Vec<u8>, DecodeError> {
    let data = general_purpose::STANDARD.decode(input)?;
    let mut decrypt_data = rsa::rsa_decrypt(&data);
    let (key1, output) = decrypt_data.split_at_mut(16);
    xor::xor_transform(output, &xor::xor_derive_key(&key1, 12));
    output.reverse();
    xor::xor_transform(output, &xor::xor_derive_key(key, 4));
    Ok(output.to_owned())
}

// only the pickcode of a single file works
pub async fn get_download_link(
    client: ClientWithMiddleware,
    pick_code: String,
) -> Result<DownloadInfo, CloudError> {
    let mut key = [0u8; 16];
    rand::rng().fill(&mut key);
    let params = json!({
        "pickcode": pick_code
    });
    let params = serde_json::to_string(&params).unwrap();
    let data = encode(params.bytes().collect(), &key);
    let mut form_data = HashMap::new();
    form_data.insert("data", data);
    let cookies = &CONFIG.load().cookies;
    let mut headers = HeaderMap::new();
    headers.insert(COOKIE, cookies.parse().unwrap());
    let response = client
        .post("https://proapi.115.com/app/chrome/downurl")
        .headers(headers)
        .query(&[(
            "t",
            std::time::SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
                .to_string(),
        )])
        .form(&form_data)
        .header(CONTENT_TYPE, "application/json")
        .send()
        .await?
        .text()
        .await?;
    // println!("{}", response);
    let result = serde_json::from_str::<DownloadResponse>(&response)?;
    if result.state {
        let data_str = decode(result.data, &key)?;
        let download_data = serde_json::from_slice::<DownloadData>(&data_str)?;
        for download_info in download_data.into_values() {
            // println!(
            //     "download file {}\n\tname: {}\n\turl: {}\n\tsize: {}",
            //     i, download_info.file_name, download_info.url.url, download_info.file_size
            // );
            return Ok(download_info);
        }
    }
    Err(format!("{}", result.msg).into())
}

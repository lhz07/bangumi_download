use crate::crypto::{rsa, xor};
use base64::{Engine, engine::general_purpose};
use rand::{self, Rng};
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
pub struct DownloadResponse {
    pub state: bool,
    pub msg: String,
    pub data: String,
}

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

pub async fn get_download_link(pick_code: String) {
    let mut key = [0u8; 16];
    rand::rng().fill(&mut key);
    let params = json!({
        "pickcode": pick_code
    });
    let _params = serde_json::to_string(&params).unwrap();
}

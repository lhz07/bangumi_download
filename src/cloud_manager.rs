use std::cell::Cell;
use serde_json::Value;
use crate::login_with_qrcode::login_with_qrcode;
use tokio_retry::{Retry, strategy::FixedInterval};

pub async fn update_cloud_cookies() -> Value{
    let try_times = Cell::new(0);
    match Retry::spawn(FixedInterval::from_millis(5000).take(4), async || {
            if try_times.get() > 0 {
                eprintln!("Failed, waiting for retry.");
            }
            try_times.set(1);
            login_with_qrcode("alipaymini").await
        }).await{
        Ok(cookies) => cookies,
        Err(error) => {
            eprintln!("Can not get cookies after retries!\nError: {error}");
            std::process::exit(1);
        }
    }
}
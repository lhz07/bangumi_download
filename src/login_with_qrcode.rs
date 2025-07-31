use crate::errors::CloudError;
use qrcode::{Color, QrCode};
use serde::{Deserialize, Serialize};

// const DEVICE: [&str; 11] = ["AppEnum", "web", "android", "ios", "linux", "mac", "windows", "tv", "alipaymini", "wechatmini", "qandroid"];

#[derive(Deserialize)]
pub struct Response<T> {
    pub data: T,
}

#[derive(Deserialize)]
pub struct Token {
    pub qrcode: String,
    pub uid: String,
    pub time: u64,
    pub sign: String,
}

#[derive(Deserialize)]
pub struct Status {
    pub status: i32,
}

#[derive(Deserialize)]
pub struct QrcodeResult {
    #[serde(rename = "cookie")]
    pub cookies: Cookies,
}

#[derive(Deserialize)]
#[allow(non_snake_case)]
pub struct Cookies {
    pub UID: String,
    pub CID: String,
    pub SEID: String,
    pub KID: String,
}

#[derive(Serialize)]
pub struct Query {
    pub uid: String,
    pub time: u64,
    pub sign: String,
}

async fn get_qrcode_token(client: reqwest::Client) -> Result<Token, CloudError> {
    let response = client
        .get("https://qrcodeapi.115.com/api/1.0/web/1.0/token/")
        .send()
        .await?
        .text()
        .await?;
    let response = serde_json::from_str::<Response<Token>>(&response)?;
    Ok(response.data)
}

async fn post_qrcode_result(
    client: reqwest::Client,
    uid: &str,
    app: &str,
) -> Result<Cookies, CloudError> {
    let url = format!("https://passportapi.115.com/app/1.0/{app}/1.0/login/qrcode/");
    let response = client
        .post(url)
        .form(&[("app", app), ("account", uid)])
        .send()
        .await?
        .text()
        .await?;
    let response = serde_json::from_str::<Response<QrcodeResult>>(&response)?;
    Ok(response.data.cookies)
}

async fn get_qrcode_status(client: reqwest::Client, query: &Query) -> Result<Status, CloudError> {
    let response = client
        .get("https://qrcodeapi.115.com/get/status/?")
        .query(query)
        .send()
        .await?
        .text()
        .await?;
    let response = serde_json::from_str::<Response<Status>>(&response)?;
    Ok(response.data)
}

pub async fn login_with_qrcode(app: &str) -> Result<String, CloudError> {
    let client = reqwest::Client::new();
    let qrcode_token = get_qrcode_token(client.clone()).await?;
    let Token {
        qrcode,
        uid,
        time,
        sign,
    } = qrcode_token;
    let query = Query {
        uid: uid.clone(),
        time,
        sign,
    };
    // println!("{}", qrcode);
    let code = QrCode::new(qrcode.as_bytes()).map_err(|e| format!("{e}"))?;
    let width = code.width();
    let mut output = String::new();

    // 每两个行压缩为一个字符（上下两个像素 -> ▀、▄、█、空格）
    for y in (0..width).step_by(2) {
        for x in 0..width {
            let top = code[(x, y)] == Color::Dark;
            let bottom = if y + 1 < width {
                code[(x, y + 1)] == Color::Dark
            } else {
                false
            };
            let ch = match (top, bottom) {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                (false, false) => ' ',
            };
            output.push(ch);
        }
        output.push('\n');
    }
    // let code_string = code
    //     .render::<char>()
    //     // .dark_color(Rgb([0, 0, 128]))
    //     // .light_color(Rgb([224, 224, 224])) // adjust colors
    //     .quiet_zone(false) // disable quiet zone (white border)
    //     .min_dimensions(1, 1)
    //     .build();
    println!("{}", output);
    loop {
        match get_qrcode_status(client.clone(), &query).await {
            Ok(status) => match status.status {
                0 => println!("[status=0] qrcode: waiting"),
                1 => println!("[status=1] qrcode: scanned"),
                2 => {
                    println!("[status=2] qrcode: signed in");
                    break;
                }
                -1 => Err("[status=-1] qrcode: expired".to_string())?,
                -2 => Err("[status=-2] qrcode: canceled".to_string())?,
                _ => Err(format!("qrcode: aborted with {}", status.status))?,
            },
            Err(error) => {
                eprintln!("Error: {error}");
                continue;
            }
        }
    }
    let cookies = post_qrcode_result(client.clone(), &uid, app).await?;
    let cookie_list = vec![
        ("UID", cookies.UID),
        ("CID", cookies.CID),
        ("SEID", cookies.SEID),
        ("KID", cookies.KID),
    ];
    let cookie_str = cookie_list
        .into_iter()
        .map(|(key, value)| format!("{}={}", key, value))
        .collect::<Vec<_>>()
        .join("; ");
    Ok(cookie_str)
}

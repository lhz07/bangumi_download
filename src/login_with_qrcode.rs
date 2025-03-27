use qrcode::QrCode;
use serde_json::Value;
use std::error::Error;


// const DEVICE: [&str; 11] = ["AppEnum", "web", "android", "ios", "linux", "mac", "windows", "tv", "alipaymini", "wechatmini", "qandroid"];

async fn get_qrcode_token(client: reqwest::Client) -> Result<Value, Box<dyn Error>> {
    let response = client
        .get("https://qrcodeapi.115.com/api/1.0/web/1.0/token/")
        .send()
        .await?
        .text()
        .await?;
    let json: Value = serde_json::from_str(&response)?;
    Ok(json)
}

async fn post_qrcode_result(client: reqwest::Client, uid: &str, app: &str) -> Result<Value, Box<dyn Error>>{
    let url = format!("https://passportapi.115.com/app/1.0/{app}/1.0/login/qrcode/");
    let response = client
        .post(url)
        .form(&[("app", app), ("account", uid)])
        .send()
        .await?
        .text()
        .await?;
    let json: Value = serde_json::from_str(&response)?;
    Ok(json)
}

async fn get_qrcode_status(
    client: reqwest::Client,
    payload: &Value,
) -> Result<Value, Box<dyn Error>> {
    let response = client
        .get("https://qrcodeapi.115.com/get/status/?")
        .query(&payload)
        .send()
        .await?
        .text()
        .await?;
    let json: Value = serde_json::from_str(&response)?;
    Ok(json)
}

pub async fn login_with_qrcode(app: &str) -> Result<Value, Box<dyn Error>> {
    let client = reqwest::Client::new();
    let mut qrcode_token = get_qrcode_token(client.clone()).await?["data"].take();
    let qrcode_value = qrcode_token["qrcode"].take();
    qrcode_token
        .as_object_mut()
        .ok_or("can not get dict!")?
        .remove("qrcode");
    let qrcode = qrcode_value.as_str().ok_or("can not find qrcode url")?;
    let code = QrCode::new(qrcode.as_bytes())?;
    let code_string = code
        .render::<char>()
        .quiet_zone(false)
        .module_dimensions(3, 1)
        .build();
    println!("{}", code_string);
    loop {
        match get_qrcode_status(client.clone(), &qrcode_token).await{
            Ok(status) => match status["data"]["status"].as_i64().ok_or("can not get status")? {
                0 => println!("[status=0] qrcode: waiting"),
                1 => println!("[status=1] qrcode: scanned"),
                2 => {println!("[status=2] qrcode: signed in"); break;}
                -1 => return Err("[status=-1] qrcode: expired".into()),
                -2 => return Err("[status=-2] qrcode: canceled".into()),
                _ => return Err(format!("qrcode: aborted with {status}").into()),
            },
            Err(error) => {eprintln!("Error: {error}");continue;}
        }
    }
    let mut result = post_qrcode_result(client.clone(), qrcode_token["uid"].as_str().unwrap(), app).await?;
    result["data"]["cookie"].is_object().then_some(()).ok_or("can not get cookies")?;
    let cookies = result["data"]["cookie"].take();
    Ok(cookies)
}

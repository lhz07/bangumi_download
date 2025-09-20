use super::*;
use crate::cloud::download::{encode, get_download_link};
use crate::cloud_manager::{download_a_folder, get_file_info, list_all_files, list_files};
use crate::config_manager::Config;
use crate::id::Id;
use crate::socket_utils::{
    AsyncReadSocketMsg, AsyncWriteSocketMsg, DownloadMsg, DownloadState, SocketPath,
};
use crate::update_rss::parse_url;
use config_manager::*;
use quick_xml::de;
use std::collections::HashMap;
use std::fs::read_to_string;
use std::sync::Arc;

// NOTICE: Global variable is shared between tests, you may use `cargo test -- --test-threads=1`
// when the tests are failed
// These tests only test part of the functions

#[tokio::test]
async fn test_get_a_magnet_link() {
    use crate::update_rss::get_a_magnet_link;
    let client = ClientBuilder::new(
        reqwest::Client::builder()
            .user_agent(PC_UA)
            .connect_timeout(Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap(),
    )
    .with(RetryTransientMiddleware::new_with_policy(
        ExponentialBackoff::builder().build_with_max_retries(5),
    ))
    .build();
    assert_eq!(
        "magnet:?xt=urn:btih:af9e3cd950cad3c3d8d345e3133cee2ecd93fd5d&tr=http%3a%2f%2ft.nyaatracker.com%2fannounce&tr=http%3a%2f%2ftracker.kamigami.org%3a2710%2fannounce&tr=http%3a%2f%2fshare.camoe.cn%3a8080%2fannounce&tr=http%3a%2f%2fopentracker.acgnx.se%2fannounce&tr=http%3a%2f%2fanidex.moe%3a6969%2fannounce&tr=http%3a%2f%2ft.acg.rip%3a6699%2fannounce&tr=https%3a%2f%2ftr.bangumi.moe%3a9696%2fannounce&tr=udp%3a%2f%2ftr.bangumi.moe%3a6969%2fannounce&tr=http%3a%2f%2fopen.acgtracker.com%3a1096%2fannounce&tr=udp%3a%2f%2ftracker.opentrackr.org%3a1337%2fannounce",
        get_a_magnet_link(
            "https://mikanime.tv/Home/Episode/af9e3cd950cad3c3d8d345e3133cee2ecd93fd5d",
            &client
        )
        .await
        .unwrap()
    );
}

#[tokio::test]
async fn test_check_rss_link_and_url_parse() {
    use crate::update_rss::check_rss_link;
    let urls = [
        "https://mikanime.tv/RSS/Bangumi?bangumiId=3644&subgroupid=1230",
        "https://mikanime.tv/RSS/Bangumi?bangumiId=3644&subgroupid=1230",
        "http://mikanime.tv/RSS/Bangumi?subgroupid=1230&bangumiId=3644",
        "mikanime.tv/RSS/Bangumi?subgroupid=1230&bangumiId=3644",
        "https://mikanime.tv/RSS/Bangumi?bangumiId=2",
        "https://mikanime.tv/RSS/Bangumi?bangumiId=2&other=1",
        "https://mikanime.tv/RSS/Bangumi?subgroupid=1&bangumiId=2",
        "https://mikanime.tv/RSS/Bangumi?bangumiId=2&subgroupid=1",
    ];
    let results = [
        Ok(()),
        Ok(()),
        Ok(()),
        Err("Invalid url!".to_string()),
        Err("Invalid url!".to_string()),
        Err("Invalid url!".to_string()),
        Err(
            "can not get correct info from the link, please check bangumiId and subgroupid! Error: missing field `item`"
                .to_string(),
        ),
        Err(
            "can not get correct info from the link, please check bangumiId and subgroupid! Error: missing field `item`"
                .to_string(),
        ),
    ];

    let client = ClientBuilder::new(
        reqwest::Client::builder()
            .user_agent(PC_UA)
            .connect_timeout(Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap(),
    )
    .with(RetryTransientMiddleware::new_with_policy(
        ExponentialBackoff::builder().build_with_max_retries(5),
    ))
    .build();
    let mut futs = Vec::new();
    for url in urls {
        futs.push(check_rss_link(url, &client));
    }
    // get the results
    let check_results = futures::future::join_all(futs).await;
    for i in 0..urls.iter().count() {
        assert_eq!(check_results[i], results[i]);
        println!("{:?}", check_results[i]);
        println!("check url success: {}", i);
    }

    // test parse_url
    let urls = [
        "https://mikanime.tv/RSS/Bangumi?bangumiId=3644&subgroupid=1230",
        "https://mikanime.tv/RSS/Bangumi?bangumiId=3644&subgroupid=1230",
        "http://mikanime.tv/RSS/Bangumi?subgroupid=1230&bangumiId=3644",
        "mikanime.tv/RSS/Bangumi?subgroupid=1230&bangumiId=3644",
        "https://mikanime.tv/RSS/Bangumi?bangumiId=2",
        "https://mikanime.tv/RSS/Bangumi?subgroupid=3&other=1",
    ];
    for (i, url) in urls.iter().enumerate() {
        let result = parse_url(url);
        println!("parse url result: {} {:?}", i, result);
        match i {
            0 | 1 | 2 => {
                // Expect Ok(("3644", "1230"))
                assert!(matches!(
                    result,
                    Ok((ref a, ref b)) if a == "3644" && b == "1230"
                ));
            }
            3 => {
                // This URL lacks scheme -> we expect Err(RssLinkParse(_))
                assert!(matches!(result, Err(CatError::RssLinkParse(_))));
            }
            4 => {
                // Missing subgroupid -> expect that specific parse error
                assert!(matches!(
                    result,
                    Err(CatError::Parse(ref msg)) if msg == "missing subgroupid"
                ));
            }
            5 => {
                // Missing bangumiId -> expect that specific parse error
                assert!(matches!(
                    result,
                    Err(CatError::Parse(ref msg)) if msg == "missing bangumiId"
                ));
            }
            _ => unreachable!(),
        }
        println!("parse url success: {}", i);
    }
}

#[test]
fn test_extract_magnet_hash() {
    use crate::cloud_manager::extract_magnet_hash;
    let magnet_links = [
        "magnet:?xt=urn:btih:40882fa906a4fe9da7b57fa53a7bd880ad3244ce&tr=http%3a%2f%2ft.nyaatracker.com%2fannounce&tr=http%3a%2f%2ftracker.kamigami.org%3a2710%2fannounce&tr=http%3a%2f%2fshare.camoe.cn%3a8080%2fannounce&tr=http%3a%2f%2fopentracker.acgnx.se%2fannounce&tr=http%3a%2f%2fanidex.moe%3a6969%2fannounce&tr=http%3a%2f%2ft.acg.rip%3a6699%2fannounce&tr=https%3a%2f%2ftr.bangumi.moe%3a9696%2fannounce&tr=udp%3a%2f%2ftr.bangumi.moe%3a6969%2fannounce&tr=http%3a%2f%2fopen.acgtracker.com%3a1096%2fannounce&tr=udp%3a%2f%2ftracker.opentrackr.org%3a1337%2fannounce",
        "magnet:?xt=urn:btih:ABCDEFGHIJKLMNOPQRSTUV234567ABCD&tr=http%3a%2f%2ft.nyaatracker.com%2fannounce",
        "magnet:?dn=test&xt=urn:btih:ABCDEFGHIJKLMNOPQRSTUVWXYZ234567&tr=udp://...",
        "magnet:?xt=urn:btih:INVALIDHASH",
    ];
    let results = [
        Some("40882fa906a4fe9da7b57fa53a7bd880ad3244ce".to_string()),
        Some("ABCDEFGHIJKLMNOPQRSTUV234567ABCD".to_string()),
        Some("ABCDEFGHIJKLMNOPQRSTUVWXYZ234567".to_string()),
        None,
    ];
    for (i, link) in magnet_links.iter().enumerate() {
        assert_eq!(extract_magnet_hash(link), results[i]);
        println!("Success: {}", i);
    }
}

#[tokio::test]
async fn test_status_iter() {
    use crate::REFRESH_NOTIFY;
    use crate::main_proc::{ConsumeSema, StatusIter};
    use std::time::Instant;
    const WAIT_TIME_LIST: [Duration; 3] = [
        Duration::from_millis(200),
        Duration::from_millis(300),
        Duration::from_millis(500),
    ];
    let mut count = 0;
    let mut wait_time = StatusIter::new(&WAIT_TIME_LIST);
    REFRESH_NOTIFY.add_permits(1);
    let timer = Instant::now();
    loop {
        let t = *wait_time.next_status();
        println!("{:?}", t);
        if count == 4 {
            break;
        }
        count += 1;
        println!("count: {}", count);
        match tokio::time::timeout(t, REFRESH_NOTIFY.consume()).await {
            Ok(_) => wait_time.reset(),
            Err(_) => continue,
        }
    }
    assert_eq!(timer.elapsed().as_secs(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn test_xml() {
    use crate::update_rss::{RSS, get_response_text};
    let client = ClientBuilder::new(
        reqwest::Client::builder()
            .user_agent(PC_UA)
            .connect_timeout(Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap(),
    )
    .with(RetryTransientMiddleware::new_with_policy(
        ExponentialBackoff::builder().build_with_max_retries(5),
    ))
    .build();
    let response = get_response_text(
        "https://mikanime.tv/RSS/Bangumi?bangumiId=3623&subgroupid=370",
        &client,
    )
    .await
    .unwrap();
    let _result = de::from_str::<RSS>(&response).unwrap();
}

#[test]
fn test_serialize_config() {
    let config_str = read_to_string("tests/config.json").unwrap();
    let _config = serde_json::from_str::<Config>(&config_str).unwrap();
}

#[tokio::test]
async fn config_test() {
    use config_manager::CONFIG;
    use std::sync::Arc;
    let mut origin_config = Config::default();
    let default_filters = [
        ("611", vec!["内封"]),
        ("583", vec!["CHT"]),
        ("570", vec!["内封"]),
        (
            "default",
            vec![
                "简繁日内封",
                "简日内封",
                "简繁内封",
                "内封",
                "简体",
                "简日",
                "简繁日",
                "简中",
                "CHS",
            ],
        ),
    ];
    let default_filters = default_filters
        .iter()
        .map(|(id, filters)| (id.to_string(), SubGroup::new_const(filters)))
        .collect::<HashMap<String, SubGroup>>();
    origin_config.filter = default_filters;
    // initial config
    let mut expect_result = origin_config.clone();
    CONFIG.store(Arc::new(origin_config));
    // launch config write thread
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
    let config_manager = tokio::spawn(config_manager::modify_config(rx));
    let mut map1 = HashMap::new();
    map1.insert(
        "610".to_string(),
        SubGroup::new(vec!["简日".to_string(), "简体".to_string()]),
    );
    map1.insert("587".to_string(), SubGroup::new(vec!["CHS".to_string()]));
    expect_result.filter.extend(map1.clone());
    let cmd = Box::new(|config: &mut Config| {
        config.filter.extend(map1.into_iter());
    });
    let msg = Message::new(cmd, None);

    let expect_result = serde_json::to_value(expect_result).unwrap();
    tx.send(msg).unwrap();

    tokio::time::sleep(Duration::from_millis(1)).await;
    drop(tx);
    config_manager.await.unwrap();
    let new_config = CONFIG.load().as_ref().clone();
    let result = serde_json::to_value(new_config).unwrap();
    assert_eq!(result, expect_result);
}

// this is an example of deadlock
#[tokio::test]
async fn deadlock() {
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::Mutex;

    let mutex = Arc::new(Mutex::new(Some("dummy")));
    let m1 = mutex.clone();

    if let Some(_) = m1.lock().await.take() {
        // lock here
        match tokio::time::timeout(Duration::from_secs(2), m1.lock()).await {
            Ok(_) => {
                println!("got lock");
                unreachable!("it should be deadlock here");
            }
            Err(e) => eprintln!("can not lock: {}", e),
        }
    }
}

#[test]
fn test_xor() {
    use crypto::xor;
    let key = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let mut input = vec![2u8, 3, 3, 3, 16, 32, 18];
    let mut buf = key.to_vec();
    buf.append(&mut input);
    xor::xor_transform(&mut buf[16..], &xor::xor_derive_key(&key, 4));
    buf[16..].reverse();
    xor::xor_transform(&mut buf[16..], &xor::XOR_CLIENT_KEY);
    println!("{:?}", buf);
}

#[test]
fn test_encode() {
    let key = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
    let input = vec![2u8, 3, 3, 3, 16, 32, 18];
    let result = encode(input, &key);
    println!("{}", result);
}

#[tokio::test]
#[ignore = "this test requires real cookies"]
async fn test_download_file() {
    let old_json = std::fs::read_to_string("config.json").expect("can not read config.json");
    let data = serde_json::from_str::<Config>(&old_json).unwrap();
    CONFIG.store(Arc::new(data));
    let client = ClientBuilder::new(
        reqwest::Client::builder()
            .user_agent(PC_UA)
            .connect_timeout(Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap(),
    )
    .with(RetryTransientMiddleware::new_with_policy(
        ExponentialBackoff::builder().build_with_max_retries(5),
    ))
    .build();
    let file = get_download_link(&client, "bzh15584ul7mt8wg4".to_string())
        .await
        .unwrap();
    println!("{:?}", file);
    // let mut storge_path = PathBuf::new();
    // storge_path.push("downloads");
    // storge_path.push(file.file_name);
    // download_file(&file.url.url, &storge_path).await.unwrap();
}

#[tokio::test]
#[ignore = "this test requires real cookies"]
async fn test_list_files() {
    let old_json = std::fs::read_to_string("config.json").expect("can not read config.json");
    let data = serde_json::from_str::<Config>(&old_json).unwrap();
    CONFIG.store(Arc::new(data));
    let client = ClientBuilder::new(
        reqwest::Client::builder()
            .user_agent(PC_UA)
            .connect_timeout(Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap(),
    )
    .with(RetryTransientMiddleware::new_with_policy(
        ExponentialBackoff::builder().build_with_max_retries(5),
    ))
    .build();
    let response = list_files(&client, "1508542093939703018", 20, 20)
        .await
        .unwrap();
    println!("{:?}", response);
}

#[tokio::test]
#[ignore = "this test requires real cookies"]
async fn test_list_all_files() {
    let old_json = std::fs::read_to_string("config.json").expect("can not read config.json");
    let data = serde_json::from_str::<Config>(&old_json).unwrap();
    CONFIG.store(Arc::new(data));
    let client = ClientBuilder::new(
        reqwest::Client::builder()
            .user_agent(PC_UA)
            .connect_timeout(Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap(),
    )
    .with(RetryTransientMiddleware::new_with_policy(
        ExponentialBackoff::builder().build_with_max_retries(5),
    ))
    .build();
    let response_count = list_files(&client, "2775190645642362861", 0, 20)
        .await
        .unwrap();
    let response = list_all_files(&client, "2775190645642362861")
        .await
        .unwrap();
    println!("{:?}", response);
    assert_eq!(response.len() as i32, response_count.count)
}

#[tokio::test]
#[ignore = "this test requires real cookies"]
async fn test_download_a_folder() {
    let old_json = std::fs::read_to_string("config.json").expect("can not read config.json");
    let data = serde_json::from_str::<Config>(&old_json).unwrap();
    CONFIG.store(Arc::new(data));
    download_a_folder("2775190645642362861", Some("test_folder"))
        .await
        .unwrap();
}

#[tokio::test]
#[ignore = "this test requires real cookies"]
async fn test_get_file_info() {
    let old_json = std::fs::read_to_string("config.json").expect("can not read config.json");
    let data = serde_json::from_str::<Config>(&old_json).unwrap();
    CONFIG.store(Arc::new(data));
    let client = ClientBuilder::new(
        reqwest::Client::builder()
            .user_agent(PC_UA)
            .connect_timeout(Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap(),
    )
    .with(RetryTransientMiddleware::new_with_policy(
        ExponentialBackoff::builder().build_with_max_retries(5),
    ))
    .build();
    let result = get_file_info(&client, "2775190645642362861").await.unwrap();
    println!("{:?}", result);
}

// #[tokio::test]
// #[ignore = "this test is slow"]
// async fn test_multi_thread_download() {
//     download_file(
//         "https://dldir1v6.qq.com/qqfile/qq/QQNT/Mac/QQ_6.9.75_250710_01.dmg",
//         Path::new("downloads/qq"),
//     )
//     .await
//     .unwrap();
// }

#[tokio::test]
async fn test_bincode() {
    let socket_path = SocketPath::new("bangumi_download_test.socket");
    let listener_path = socket_path.clone();
    let old_id = Id::generate();
    let msg = ServerMsg::Download(DownloadMsg {
        id: old_id,
        state: socket_utils::DownloadState::Downloading(2233),
    });
    let listener_handle = tokio::spawn(async move {
        let listener = listener_path.to_listener().unwrap();
        let (mut stream, _) = listener.accept().await.unwrap();
        let msg = stream.read_msg().await.unwrap();
        println!("{:?}", msg);
        drop(listener);
        msg
    });
    let sender = tokio::spawn(async move {
        let mut stream = socket_path.to_stream().await.unwrap();
        stream.write_msg(msg).await.unwrap();
    });
    sender.await.unwrap();
    let listener_result = listener_handle.await.unwrap();
    if let ServerMsg::Download(download_msg) = listener_result {
        let DownloadMsg { id, state } = download_msg;
        assert_eq!(id, old_id);
        if let DownloadState::Downloading(p) = state {
            assert_eq!(p, 2233);
        } else {
            panic!()
        }
    } else {
        panic!()
    }
}

#[test]
fn test_for_unsafe_popup_handle() {
    #[derive(Default)]
    struct App {
        current_popup: Option<Popup>,
        rss_data: Vec<String>,
    }
    struct ActionConfirm {
        action: Mem,
    }
    struct Mem(Box<dyn FnMut(&mut App)>);
    impl Drop for Mem {
        fn drop(&mut self) {
            println!("dropping mem...");
        }
    }
    impl Drop for ActionConfirm {
        fn drop(&mut self) {
            println!("dropping action confirm...");
        }
    }
    enum Popup {
        _Other,
        Confirm(ActionConfirm),
    }
    let mut test_app = App::default();
    let app = &mut test_app;
    app.rss_data = vec!["rss".to_string(), "link".to_string()];
    let rss_result = vec!["rss".to_string()];
    app.current_popup = Some(Popup::Confirm(ActionConfirm {
        action: Mem(Box::new(|app| {
            app.rss_data.pop();
        })),
    }));
    if app.current_popup.is_some() {
        let popup = unsafe { app.current_popup.as_mut().unwrap_unchecked() };
        if matches!(popup, Popup::Confirm(_)) {
            let mut confirm = unsafe {
                match app.current_popup.take().unwrap_unchecked() {
                    Popup::Confirm(confirm) => confirm,
                    _ => core::hint::unreachable_unchecked(),
                }
            };
            (confirm.action.0)(app);
        }
    }
    assert!(matches!(app.current_popup, None));
    assert_eq!(app.rss_data, rss_result);
}

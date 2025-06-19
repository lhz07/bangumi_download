use std::{collections::HashMap, fs::read_to_string};

use crate::config_manager::Config;
use config_manager::*;
use super::*;
use quick_xml::de;
use serde_json::Value;

// NOTICE: Global variable is shared between tests, you may use `cargo test -- --test-threads=1`
// when the tests are failed
// These tests only test part of the functions

#[tokio::test]
async fn test_get_a_magnet_link() {
    use crate::update_rss::get_a_magnet_link;
    assert_eq!(
        Some(
            "magnet:?xt=urn:btih:af9e3cd950cad3c3d8d345e3133cee2ecd93fd5d&tr=http%3a%2f%2ft.nyaatracker.com%2fannounce&tr=http%3a%2f%2ftracker.kamigami.org%3a2710%2fannounce&tr=http%3a%2f%2fshare.camoe.cn%3a8080%2fannounce&tr=http%3a%2f%2fopentracker.acgnx.se%2fannounce&tr=http%3a%2f%2fanidex.moe%3a6969%2fannounce&tr=http%3a%2f%2ft.acg.rip%3a6699%2fannounce&tr=https%3a%2f%2ftr.bangumi.moe%3a9696%2fannounce&tr=udp%3a%2f%2ftr.bangumi.moe%3a6969%2fannounce&tr=http%3a%2f%2fopen.acgtracker.com%3a1096%2fannounce&tr=udp%3a%2f%2ftracker.opentrackr.org%3a1337%2fannounce"
        ),
        get_a_magnet_link(
            "https://mikanime.tv/Home/Episode/af9e3cd950cad3c3d8d345e3133cee2ecd93fd5d"
        )
        .await
        .as_deref()
    );
}

#[tokio::test]
async fn test_check_rss_link() {
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
    let mut futs = Vec::new();
    for url in urls {
        futs.push(check_rss_link(url));
    }
    // get the results
    let check_results = futures::future::join_all(futs).await;
    for i in 0..urls.iter().count() {
        assert_eq!(check_results[i], results[i]);
        println!("{:?}", check_results[i]);
        println!("Success: {}", i);
    }
}

#[test]
fn test_extract_magnet_hash() {
    use crate::cloud_manager::extract_magnet_hash;
    let magnet_links = [
        "magnet:?xt=urn:btih:40882fa906a4fe9da7b57fa53a7bd880ad3244ce&tr=http%3a%2f%2ft.nyaatracker.com%2fannounce&tr=http%3a%2f%2ftracker.kamigami.org%3a2710%2fannounce&tr=http%3a%2f%2fshare.camoe.cn%3a8080%2fannounce&tr=http%3a%2f%2fopentracker.acgnx.se%2fannounce&tr=http%3a%2f%2fanidex.moe%3a6969%2fannounce&tr=http%3a%2f%2ft.acg.rip%3a6699%2fannounce&tr=https%3a%2f%2ftr.bangumi.moe%3a9696%2fannounce&tr=udp%3a%2f%2ftr.bangumi.moe%3a6969%2fannounce&tr=http%3a%2f%2fopen.acgtracker.com%3a1096%2fannounce&tr=udp%3a%2f%2ftracker.opentrackr.org%3a1337%2fannounce",
        "magnet:?xt=urn:btih:ABCDEFGHIJKLMNOPQRSTUV234567ABCD&tr=http%3a%2f%2ft.nyaatracker.com%2fannounce",
        "magnet:?xt=urn:btih:INVALIDHASH",
    ];
    let results = [
        Some("40882fa906a4fe9da7b57fa53a7bd880ad3244ce".to_string()),
        Some("ABCDEFGHIJKLMNOPQRSTUV234567ABCD".to_string()),
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
    use crate::main_proc::ConsumeSema;
    use crate::main_proc::StatusIter;
    use std::time::Instant;
    const WAIT_TIME_LIST: [Duration; 3] = [
        Duration::from_millis(200),
        Duration::from_millis(300),
        Duration::from_millis(500),
    ];
    let mut count = 0;
    let mut wait_time = StatusIter::new(&WAIT_TIME_LIST);
    let reset_wait_time = REFRESH_NOTIFY.lock().await.clone();
    reset_wait_time.add_permits(1);
    let timer = Instant::now();
    loop {
        let t = *wait_time.next().unwrap();
        println!("{:?}", t);
        if count == 4 {
            break;
        }
        count += 1;
        println!("count: {}", count);
        match tokio::time::timeout(t, reset_wait_time.consume()).await {
            Ok(_) => wait_time.reset(),
            Err(_) => continue,
        }
    }
    assert_eq!(timer.elapsed().as_secs(), 1);
}

#[tokio::test]
async fn test_xml() {
    use crate::update_rss::RSS;
    use crate::update_rss::get_response_text;
    let response = get_response_text(
        "https://mikanime.tv/RSS/Bangumi?bangumiId=3623&subgroupid=370",
        CLIENT_WITH_RETRY.clone(),
    )
    .await
    .unwrap();
    let result = de::from_str::<RSS>(&response);
    assert!(result.is_ok());
}

#[test]
fn test_serialize_config() {
    let config_str = read_to_string("config.json").unwrap();
    let config = serde_json::from_str::<Config>(&config_str);
    assert!(config.is_ok());
    // println!("{:#?}", config);
}

fn general_config_modify_test(origin: Value, msg: Message) -> Config {
    let mut config = serde_json::from_value::<Config>(origin).unwrap();
    println!("{:#?}", config);
    match msg.cmd {
        MessageCmd::Replace => msg.value.replace(msg.keys, &mut config),
        MessageCmd::Append => msg.value.add(msg.keys, &mut config),
        MessageCmd::Delete => msg.value.remove(msg.keys, &mut config),
    }
    println!("{:#?}", config);
    config
}

#[tokio::test]
async fn config_test() {
    use config_manager::CONFIG;
    let origin = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"], 
            "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "hash_ani": {}, "hash_ani_slow": {}});
    // initial config
    let origin_config = serde_json::from_value::<Config>(origin).unwrap();
    *CONFIG.write().await.get_mut() = origin_config;
    // launch config write thread
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
    let config_manager = tokio::spawn(config_manager::modify_config(rx));
    let mut map1 = HashMap::<String, Vec<String>>::new();
    map1.insert(
        "610".to_string(),
        vec!["简日".to_string(), "简体".to_string()],
    );
    map1.insert("587".to_string(), vec!["CHS".to_string()]);
    let msg = Message::new(
        vec!["filter".to_string()],
        config_manager::MessageType::MapVec(map1),
        config_manager::MessageCmd::Append,
        None,
    );
    let expect_result = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"], "610": ["简日", "简体"], "587": ["CHS"],
            "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "hash_ani": {}, "hash_ani_slow": {}});
    tx.send(msg).unwrap();

    tokio::time::sleep(Duration::from_millis(1)).await;
    drop(tx);
    config_manager.await.unwrap();
    let new_config = CONFIG.read().await.get().clone();
    let result = serde_json::to_value(new_config).unwrap();
    assert_eq!(result, expect_result);
}


#[test]
fn replace_vec() {
    let origin = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"], 
            "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "hash_ani": {}, "hash_ani_slow": {}});

    let msg = Message::new(
        vec!["filter".to_string(), "default".to_string()],
        MessageType::List(vec!["简日内嵌".to_string()]),
        MessageCmd::Replace,
        None,
    );
    let expect_result = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"], 
            "default": ["简日内嵌"]}, 
        "magnets":{}, "hash_ani": {}, "hash_ani_slow": {}});
    let new_config = serde_json::to_value(general_config_modify_test(origin, msg)).unwrap();
    assert_eq!(new_config, expect_result);
    // match_type!(cookies,
    //         String => {println!("String!")},
    //         Vec<String> => {println!("Vec<String>!")}
    // );
}

#[test]
fn replace_text() {
    let origin = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"], 
            "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "hash_ani": {}, "hash_ani_slow": {}});

    let msg = Message::new(
        vec!["user".to_string(), "name".to_string()],
        config_manager::MessageType::Text("master".to_string()),
        config_manager::MessageCmd::Replace,
        None,
    );
    let expect_result = serde_json::json!(
        {"user":{"name":"master", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"], 
            "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "hash_ani": {}, "hash_ani_slow": {}});
    let new_config = general_config_modify_test(origin, msg);
    let result = serde_json::to_value(new_config).unwrap();
    assert_eq!(expect_result, result);
}

#[test]
fn append_vec_to_vec() {
    let origin = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"], 
            "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "hash_ani": {}, "hash_ani_slow": {}});

    let msg = Message::new(
        vec!["filter".to_string(), "611".to_string()],
        config_manager::MessageType::List(vec!["简日内嵌".to_string(), "CHS".to_string()]),
        config_manager::MessageCmd::Append,
        None,
    );
    let expect_result = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封", "简日内嵌", "CHS"], "583": ["CHT"], "570": ["内封"], 
            "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "hash_ani": {}, "hash_ani_slow": {}});
    let new_config = general_config_modify_test(origin, msg);
    let result = serde_json::to_value(new_config).unwrap();
    assert_eq!(expect_result, result);
}

#[test]
fn append_map() {
    let origin = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"], 
            "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "hash_ani": {}, "hash_ani_slow": {}});

    let mut map1 = HashMap::<String, Vec<String>>::new();
    map1.insert(
        "610".to_string(),
        vec!["简日".to_string(), "简体".to_string()],
    );
    map1.insert("587".to_string(), vec!["CHS".to_string()]);
    let msg = Message::new(
        vec!["filter".to_string()],
        config_manager::MessageType::MapVec(map1),
        config_manager::MessageCmd::Append,
        None,
    );
    let expect_result = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"], "610": ["简日", "简体"], "587": ["CHS"],
            "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "hash_ani": {}, "hash_ani_slow": {}});
    let new_config = general_config_modify_test(origin, msg);
    let result = serde_json::to_value(new_config).unwrap();
    assert_eq!(expect_result, result);
}

#[test]
fn append_text_to_vec() {
    let origin = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"], 
            "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "hash_ani": {}, "hash_ani_slow": {}});

    let msg = Message::new(
        vec!["filter".to_string(), "611".to_string()],
        config_manager::MessageType::Text("简日内嵌".to_string()),
        config_manager::MessageCmd::Append,
        None,
    );
    let expect_result = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封", "简日内嵌"], "583": ["CHT"], "570": ["内封"], 
            "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "hash_ani": {}, "hash_ani_slow": {}});
    let new_config = general_config_modify_test(origin, msg);
    let result = serde_json::to_value(new_config).unwrap();
    assert_eq!(expect_result, result);
}

#[test]
fn del_key() {
    let origin = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"], "233": ["繁体"],
            "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "hash_ani": {}, "hash_ani_slow": {}});

    let msg = Message::new(
        vec!["filter".to_string(), "233".to_string()],
        config_manager::MessageType::None,
        config_manager::MessageCmd::Delete,
        None,
    );
    let expect_result = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"], 
            "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "hash_ani": {}, "hash_ani_slow": {}});
    let new_config = general_config_modify_test(origin, msg);
    let result = serde_json::to_value(new_config).unwrap();
    assert_eq!(expect_result, result);
}

#[test]
fn del_value() {
    let origin = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"],
            "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "hash_ani": {}, "hash_ani_slow": {}});

    let msg = Message::new(
        vec!["filter".to_string(), "default".to_string()],
        config_manager::MessageType::Text("简体".to_string()),
        config_manager::MessageCmd::Delete,
        None,
    );
    let expect_result = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"], 
            "default": ["简繁日内封", "简日内封", "简繁内封", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "hash_ani": {}, "hash_ani_slow": {}});
    let new_config = general_config_modify_test(origin, msg);
    let result = serde_json::to_value(new_config).unwrap();
    assert_eq!(expect_result, result);
}
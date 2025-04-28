use super::*;
use serde_json::Value;

// NOTICE: Global variable is shared between tests, so use `cargo test -- --test-threads=1`
// These tests only test for config_modify and get_a_magnet_link.

async fn general_config_test(origin: Value, msg: Message) -> Value {
    use config_manager::CONFIG;
    // initial config
    *CONFIG.write().await.get_mut_value() = origin;
    // launch config write thread
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Message>();
    let config_manager = tokio::spawn(config_manager::modify_config(rx));
    tx.send(msg).unwrap();

    tokio::time::sleep(Duration::from_millis(1)).await;
    drop(tx);
    config_manager.await.unwrap();
    CONFIG.read().await.get_value().clone()
}

#[tokio::test]
async fn append_map() {
    let origin = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"], 
            "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "downloading_hash": [], "hash_ani": {}, "temp": {}, "files_to_download": {}});

    let map1 = serde_json::json!({"610": ["简日", "简体"], "587": ["CHS"]})
        .as_object()
        .unwrap()
        .clone();
    let msg = Message::new(
        vec!["filter".to_string()],
        config_manager::MessageType::Map(map1),
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
        "magnets":{}, "downloading_hash": [], "hash_ani": {}, "temp": {}, "files_to_download": {}});
    let result = general_config_test(origin, msg).await;
    assert_eq!(expect_result, result);
}

#[tokio::test]
async fn append_text_to_vec() {
    let origin = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"], 
            "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "downloading_hash": [], "hash_ani": {}, "temp": {}, "files_to_download": {}});

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
        "magnets":{}, "downloading_hash": [], "hash_ani": {}, "temp": {}, "files_to_download": {}});
    let result = general_config_test(origin, msg).await;
    assert_eq!(expect_result, result);
}

#[tokio::test]
async fn del_key() {
    let origin = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"], "233": ["繁体"],
            "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "downloading_hash": [], "hash_ani": {}, "temp": {}, "files_to_download": {}});

    let msg = Message::new(
        vec!["filter".to_string(), "233".to_string()],
        config_manager::MessageType::None,
        config_manager::MessageCmd::DeleteKey,
        None,
    );
    let expect_result = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"], 
            "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "downloading_hash": [], "hash_ani": {}, "temp": {}, "files_to_download": {}});
    let result = general_config_test(origin, msg).await;
    assert_eq!(expect_result, result);
}

#[tokio::test]
async fn del_value() {
    let origin = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"],
            "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "downloading_hash": [], "hash_ani": {}, "temp": {}, "files_to_download": {}});

    let msg = Message::new(
        vec!["filter".to_string(), "default".to_string()],
        config_manager::MessageType::Text("简体".to_string()),
        config_manager::MessageCmd::DeleteValue,
        None,
    );
    let expect_result = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"], 
            "default": ["简繁日内封", "简日内封", "简繁内封", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "downloading_hash": [], "hash_ani": {}, "temp": {}, "files_to_download": {}});
    let result = general_config_test(origin, msg).await;
    assert_eq!(expect_result, result);
}

#[tokio::test]
async fn append_vec_to_vec() {
    let origin = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"], 
            "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "downloading_hash": [], "hash_ani": {}, "temp": {}, "files_to_download": {}});

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
        "magnets":{}, "downloading_hash": [], "hash_ani": {}, "temp": {}, "files_to_download": {}});
    let result = general_config_test(origin, msg).await;
    assert_eq!(expect_result, result);
}

#[tokio::test]
async fn replace_vec() {
    let origin = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"], 
            "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "downloading_hash": [], "hash_ani": {}, "temp": {}, "files_to_download": {}});

    let msg = Message::new(
        vec!["filter".to_string(), "default".to_string()],
        config_manager::MessageType::List(vec!["简日内嵌".to_string()]),
        config_manager::MessageCmd::Replace,
        None,
    );
    let expect_result = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"], 
            "default": ["简日内嵌"]}, 
        "magnets":{}, "downloading_hash": [], "hash_ani": {}, "temp": {}, "files_to_download": {}});
    let result = general_config_test(origin, msg).await;
    assert_eq!(expect_result, result);
}

#[tokio::test]
async fn replace_text() {
    let origin = serde_json::json!(
        {"user":{"name":"", "password": ""},
        "bangumi":{}, "cookies": "", 
        "rss_links": {}, 
        "filter": {
            "611": ["内封"], "583": ["CHT"], "570": ["内封"], 
            "default": ["简繁日内封", "简日内封", "简繁内封", "简体", "简日", "简繁日", "简中", "CHS"]}, 
        "magnets":{}, "downloading_hash": [], "hash_ani": {}, "temp": {}, "files_to_download": {}});

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
        "magnets":{}, "downloading_hash": [], "hash_ani": {}, "temp": {}, "files_to_download": {}});
    let result = general_config_test(origin, msg).await;
    assert_eq!(expect_result, result);
}

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

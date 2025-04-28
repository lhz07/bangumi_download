use std::{collections::HashMap, time::Duration, vec};

use crate::{
    REFRESH_DOWNLOAD, TX,
    alist_manager::{download_a_task, get_alist_name_passwd, get_alist_token},
    cloud_manager::{del_cloud_task, get_tasks_list},
    config_manager::{CONFIG, Message, MessageCmd, MessageType},
    update_rss::start_rss_receive,
};

pub async fn refresh_rss() {
    loop {
        println!("\nChecking updates...\n");
        let rss_links = CONFIG.read().await.get_value()["rss_links"].clone();
        let username = CONFIG.read().await.get_value()["user"]["name"]
            .as_str()
            .unwrap()
            .to_string();
        let password = CONFIG.read().await.get_value()["user"]["password"]
            .as_str()
            .unwrap()
            .to_string();
        let urls = rss_links
            .as_object()
            .unwrap()
            .iter()
            .map(|(_, link)| link.as_str().unwrap())
            .collect::<Vec<_>>();
        start_rss_receive(urls).await;
        println!("\nCheck finished!\n");
        tokio::time::sleep(Duration::from_secs(2700)).await;
        // update alist token
        if let Err(error) = get_alist_token(&username, &password).await {
            eprintln!("Error occured when trying to get alist token: {}", error);
            eprintln!("Do you want to change alist username and password? [y/n]");
            let mut input = String::new();
            std::io::stdin()
                .read_line(&mut input)
                .expect("Failed to read username!");
            let select = input.trim();
            if select == "y" {
                let (name, password) = get_alist_name_passwd().await;
                let tx = TX.read().await.clone().unwrap();
                let msg = Message::new(
                    vec!["user".to_string(), "name".to_string()],
                    MessageType::Text(name),
                    MessageCmd::Replace,
                    None,
                );
                tx.send(msg).unwrap();
                let msg = Message::new(
                    vec!["user".to_string(), "password".to_string()],
                    MessageType::Text(password),
                    MessageCmd::Replace,
                    None,
                );
                tx.send(msg).unwrap();
            }
        }
    }
}

pub async fn refresh_download() {
    let mut wait_time_list = [30, 60, 120, 300, 600, 1200, 1800].into_iter();
    let mut error_task = HashMap::new();
    'outer: loop {
        if CONFIG.read().await.get_value()["downloading_hash"]
            .as_array()
            .unwrap()
            .is_empty()
        {
            break;
        }
        let hash_ani = CONFIG.read().await.get_value()["hash_ani"].clone();
        let tasks_list = match get_tasks_list().await {
            Ok(list) => list,
            Err(error) => {
                eprintln!("Error occurred when attempting to obtain the task list: {error}");
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            }
        };
        for task in tasks_list {
            if task["percentDone"] == 100 {
                // download file
                let file_name = task["name"].as_str().unwrap().to_string();
                let ani_name = hash_ani[task["info_hash"].as_str().unwrap()]
                    .as_str()
                    .unwrap()
                    .to_string();
                let path = format!("/115/云下载/{file_name}/{file_name}");
                if let Err(error) = download_a_task(&path, &ani_name).await {
                    eprintln!("Can not download a task, error: {}", error);
                    error_task
                        .entry(task["info_hash"].as_str().unwrap().to_string())
                        .and_modify(|times| *times += 1)
                        .or_insert(1);
                    if error_task[task["info_hash"].as_str().unwrap()] > 3 {
                        break 'outer;
                    }
                    tokio::time::sleep(Duration::from_secs(60)).await;
                    continue;
                }
                // after download
                println!("Task {} is finished! Deleting task!", task["name"]);
                del_cloud_task(task["info_hash"].as_str().unwrap())
                    .await
                    .unwrap();
                {
                    let tx = TX.read().await.clone().unwrap();
                    let msg = Message::new(
                        vec!["downloading_hash".to_string()],
                        MessageType::Text(task["info_hash"].as_str().unwrap().to_string()),
                        MessageCmd::DeleteValue,
                        None,
                    );
                    tx.send(msg).unwrap();
                    let msg = Message::new(
                        vec!["hash_ani".to_string()],
                        MessageType::Text(task["info_hash"].as_str().unwrap().to_string()),
                        MessageCmd::DeleteKey,
                        None,
                    );
                    tx.send(msg).unwrap();
                }
            }
        }
        match wait_time_list.next() {
            Some(secs) => tokio::time::sleep(Duration::from_secs(secs)).await,
            None => tokio::time::sleep(Duration::from_secs(3600)).await,
        }
    }
}

pub async fn restart_refresh_download() {
    if let Some(_) = REFRESH_DOWNLOAD.lock().await.take_if(|h| h.is_finished()) {
        let download_handle = tokio::spawn(refresh_download());
        REFRESH_DOWNLOAD.lock().await.replace(download_handle);
    }
}

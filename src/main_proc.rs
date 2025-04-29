use std::{collections::HashMap, sync::Arc, time::Duration, vec};

use crate::{
    REFRESH_DOWNLOAD, REFRESH_NOTIFY, TX,
    alist_manager::{
        check_is_alist_working, download_a_task, get_alist_name_passwd, get_alist_token,
    },
    cloud_manager::{del_cloud_task, get_tasks_list},
    config_manager::{CONFIG, Message, MessageCmd, MessageType},
    update_rss::start_rss_receive,
};

struct StatusIter<'a, T> {
    index: usize,
    data: &'a [T],
}

impl<'a, T: Clone> StatusIter<'a, T> {
    fn new(data: &'a [T]) -> Self {
        Self { index: 0, data }
    }
    fn reset(&mut self) {
        self.index = 0;
    }
}

impl<'a, T: Clone> Iterator for StatusIter<'a, T> {
    type Item = &'a T;
    fn next(&mut self) -> Option<Self::Item> {
        if self.index < self.data.len() {
            let next_item = &self.data[self.index];
            self.index += 1;
            Some(next_item)
        } else if self.index == self.data.len() {
            Some(&self.data[self.index])
        } else {
            None
        }
    }
}

pub async fn refresh_rss() {
    'outer: loop {
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
        // check is alist working
        if let Err(error) = check_is_alist_working().await {
            eprintln!("{error}");
            println!("Rss refresh is stopped!");
            break;
        }
        // update alist token
        if let Err(error) = get_alist_token(&username, &password).await {
            loop {
                eprintln!("Error occured when trying to get alist token: {}", error);
                println!("Do you want to change alist username and password? [y/n]");
                let mut input = String::new();
                std::io::stdin()
                    .read_line(&mut input)
                    .expect("Failed to read username!");
                let select = input.trim();
                match select {
                    "y" => {
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
                        break;
                    }
                    "n" => {
                        println!("Rss refresh is stopped!");
                        break 'outer;
                    }
                    _ => {
                        println!("Invalid input, please type 'y' or 'n'");
                        continue;
                    }
                }
            }
        }
    }
}

pub async fn refresh_download() {
    const WAIT_TIME_LIST: [Duration; 6] = [
        Duration::from_secs(60),
        Duration::from_secs(60),
        Duration::from_secs(120),
        Duration::from_secs(120),
        Duration::from_secs(300),
        Duration::from_secs(600),
    ];
    let mut wait_time = StatusIter::new(&WAIT_TIME_LIST);
    let mut error_task = HashMap::new();
    
    let reset_wait_time = REFRESH_NOTIFY.lock().await.clone();
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
            // download task failed, delete it
            if task["status"] == -1{
                del_cloud_task(task["info_hash"].as_str().unwrap())
                    .await
                    .unwrap();
            }
            if task["percentDone"] == 100 {
                // download file
                let file_name = task["name"].as_str().unwrap().to_string();
                let ani_name = hash_ani[task["info_hash"].as_str().unwrap()]
                    .as_str()
                    .unwrap()
                    .to_string();
                let path = format!("/115/云下载/{file_name}/{file_name}");
                // check is alist working
                if let Err(error) = check_is_alist_working().await {
                    eprintln!("{error}");
                    println!("Download refresh is stopped!");
                    break;
                }
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
                        vec![
                            "hash_ani".to_string(),
                            task["info_hash"].as_str().unwrap().to_string(),
                        ],
                        MessageType::None,
                        MessageCmd::DeleteKey,
                        None,
                    );
                    tx.send(msg).unwrap();
                }
            }
        }
        match tokio::time::timeout(
            *wait_time.next().unwrap(),
            reset_wait_time.acquire(),
        )
        .await
        {
            Ok(_) => wait_time.reset(),
            Err(_) => continue,
        }
    }
}

pub async fn restart_refresh_download() {
    REFRESH_NOTIFY.lock().await.add_permits(1);
    if let Some(_) = REFRESH_DOWNLOAD.lock().await.take_if(|h| h.is_finished()) {
        let download_handle = tokio::spawn(refresh_download());
        REFRESH_DOWNLOAD.lock().await.replace(download_handle);
    }
}

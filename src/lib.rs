use quick_xml::{Reader, events::Event};
use rss::Channel;
use scraper::{Html, Selector};
use serde_json::{Map, Value};
use std::{error::Error, str::FromStr};

fn parse_rss_to_dict(xml: &str) -> Vec<(String, Value)> {
    let mut reader = Reader::from_str(xml);
    // println!("{xml}");
    let mut buf = Vec::new();
    let mut stack: Vec<(String, Value)> = Vec::new();
    stack.push(("root".to_string(), Value::Object(Map::new())));
    // let mut value;

    while let Ok(event) = reader.read_event_into(&mut buf) {
        match event {
            Event::Start(e) => {
                let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                // println!("create: {:?}", tag_name);
                stack.push((tag_name.clone(), Value::Object(Map::new())));
            }
            Event::Text(e) => {
                let text = e.unescape().unwrap().to_string();
                // println!("content: {:?}", text);
                if let Some((_, last_value)) = stack.last_mut() {
                    // println!("push content: {:?}", text);
                    *last_value = Value::String(text);
                }
            }
            Event::End(e) => {
                let tag_name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                // println!("end: {:?}", tag_name);
                // println!("{:?}", tag_name);
                // println!("{:?}", stack.last());
                if tag_name == "item" {
                    if let Some((_, value)) = stack.pop(){
                        if let Some((_, parent)) = stack.last_mut() {
                            if let Value::Object(map) = parent {
                                map.entry("item".to_string()).or_insert_with(||Value::Array(Vec::new()))
                                    .as_array_mut()
                                    .unwrap()
                                    .push(value);
                            }
                        }
                    }
                } else if let Some((_, value)) = stack.pop() {
                    // println!("value: {:?}", value);
                    if let Some((_, parent)) = stack.last_mut() {
                        // println!("parent: {:?}", parent);
                        if let Value::Object(map) = parent {
                            // println!("insert {} and {} into map: {:?}", tag_name, value, map);
                            map.insert(tag_name, value);
                        }
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    println!("{:?}", stack);
    stack
}

async fn get_response_text(url: &str) -> Result<String, reqwest::Error> {
    Ok(reqwest::get(url).await?.text().await?)
}

async fn get_subgroup_name(url: &str) -> Option<String> {
    let response = match get_response_text(url).await {
        Ok(response) => response,
        Err(error) => {
            eprintln!("can not open {url}, error: {error}");
            return None;
        }
    };
    let resource = Html::parse_document(&response);
    let selector = Selector::parse("a.magnet-link-wrap").unwrap();
    let sub_name = resource.select(&selector).next()?.text().next()?;
    Some(sub_name.to_string())
}

pub async fn rss_receive(url: &str) -> Result<(), Box<dyn Error>> {
    let response = reqwest::get(url).await?.text().await?;
    let xml_parser = parse_rss_to_dict(&response);
    println!("{:?}", xml_parser);
    let rss_content = Channel::from_str(&response)?;
    let latest_item = rss_content
        .items()
        .first()
        .ok_or("can not found latest item!")?;
    let mut split_ani_sub = rss_content
        .link
        .split("bangumiId=")
        .nth(1)
        .ok_or("can not find ani_id and sub_id!")?
        .split("&subgroupid=");
    let ani_id = split_ani_sub
        .next()
        .ok_or("can not found ani_id!")?
        .to_string();
    let sub_id = split_ani_sub
        .next()
        .ok_or("can not found sub_id!")?
        .to_string();
    let sub_name = get_subgroup_name(latest_item.link().ok_or("can not found link!")?)
        .await
        .unwrap_or_default();
    let bangumi_name = rss_content
        .title()
        .split(" - ")
        .nth(1)
        .unwrap_or(rss_content.title())
        .to_string();
    let title = format!("[{sub_name}] {bangumi_name}");
    let last_update = latest_item.pub_date();
    println!("{} {} {} {:?}", ani_id, sub_id, title, last_update);
    // for i in rss_content.into_items() {
    //     println!("{}", &i.title().expect("MUST HAVE TITLE!"));
    // }
    Ok(())
}

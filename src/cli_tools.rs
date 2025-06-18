use crate::{socket_utils::{SocketListener, SocketStream}, TX};
use clap::{Parser, Subcommand};
use tokio::task::JoinHandle;
#[derive(Parser)]
#[command(name = "unix-socket", version = "1.0", author = "lhz")]
pub struct Args {
    // #[clap(long="no-cursor", default_value = "true", action=ArgAction::SetFalse, help="don't capture the cursor")]
    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    #[clap(name = "update", about = "更新RSS订阅")]
    Update,
    #[clap(name = "add-link", about = "添加RSS链接")]
    AddLink { link: String },
    #[clap(name = "del-link", about = "删除RSS链接")]
    DelLink,
}

pub struct Cli;

impl Cli {
    pub async fn add_a_link(stream: &mut SocketStream, link: Option<&str>) {
        let mut input = String::new();
        let rss_link = match link {
            Some(link) => link,
            None => {
                println!("请输入要添加的RSS链接:");
                std::io::stdin().read_line(&mut input).unwrap();
                input.trim()
            }
        };
        if rss_link.is_empty() {
            println!("RSS链接不能为空");
            return;
        }
        stream.write_str("add-link").await.unwrap();
        stream.write_str(rss_link).await.unwrap();
        stream.read_str_to_end().await.unwrap();
        // println!("stream: {}", stream.read_str().await.unwrap());
    }
    pub async fn del_a_link(_stream: &mut SocketStream) {}
    pub async fn add_subgroup_filter(_stream: &mut SocketStream) {}
    pub async fn del_subgroup_filter(_stream: &mut SocketStream) {}
    pub async fn add_single_magnet_download(_stream: &mut SocketStream) {}

    pub async fn update(stream: &mut SocketStream) {
        stream.write_str("Update!").await.unwrap();
        println!("stream: {}", stream.read_str().await.unwrap());
    }
    pub async fn exit() {
        
    }
}

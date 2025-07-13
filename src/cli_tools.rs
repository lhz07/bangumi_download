use crate::socket_utils::SocketStream;
use clap::{Parser, Subcommand};
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

pub const ADD_LINK: &str = "add-link";
pub const DOWNLOAD_FOLDER: &str = "download-folder";

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
        stream.write_str(ADD_LINK).await.unwrap();
        stream.write_str(rss_link).await.unwrap();
        stream.read_str_to_end().await.unwrap();
    }
    pub async fn del_a_link(_stream: &mut SocketStream) {}
    pub async fn add_subgroup_filter(_stream: &mut SocketStream) {}
    pub async fn del_subgroup_filter(_stream: &mut SocketStream) {}
    pub async fn add_single_magnet_download(_stream: &mut SocketStream) {}
    pub async fn download_a_folder(stream: &mut SocketStream) {
        let mut input = String::new();
        println!("请输入要下载的文件夹cid:");
        std::io::stdin().read_line(&mut input).unwrap();
        let cid = input.trim();
        if cid.is_empty() {
            println!("cid不能为空");
            return;
        }
        stream.write_str(DOWNLOAD_FOLDER).await.unwrap();
        stream.write_str(cid).await.unwrap();
        stream.read_str_to_end().await.unwrap();
    }

    pub async fn update(stream: &mut SocketStream) {
        stream.write_str("Update!").await.unwrap();
        println!("stream: {}", stream.read_str().await.unwrap());
    }
    pub async fn exit() {}
}

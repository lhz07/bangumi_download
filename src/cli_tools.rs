use std::ops::{Deref, DerefMut};

use crate::socket_utils::{SocketPath, SocketStream};
use clap::{Parser, Subcommand};
use tokio::io;

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

pub struct Cli {
    stream: SocketStream,
}

impl Deref for Cli {
    type Target = SocketStream;
    fn deref(&self) -> &Self::Target {
        &self.stream
    }
}

impl DerefMut for Cli {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.stream
    }
}

impl Cli {
    pub fn new(stream: SocketStream) -> Self {
        Cli { stream }
    }
    pub async fn cli_main(socket_path: SocketPath) -> Result<(), io::Error> {
        let arg = Args::parse();
        let stream = socket_path.to_stream().await?;
        let mut cli = Cli::new(stream);
        match arg.command {
            Some(cmd) => {
                cli.write_str("short").await?;
                match cmd {
                    Command::Update => cli.update().await?,
                    Command::AddLink { link } => cli.add_a_link(Some(&link)).await?,
                    Command::DelLink => cli.del_a_link().await,
                }
            }
            None => {
                cli.write_str("keep-alive").await?;
                loop {
                    println!(
                        "\n请输入想要执行的操作: \n1.添加RSS链接\n2.删除RSS链接\n3.添加字幕组过滤器\n4.删除字幕组过滤器\n5.添加单个磁链下载\n6.下载文件夹\n7.退出程序\n"
                    );
                    let mut input = String::new();
                    std::io::stdin().read_line(&mut input)?;
                    let select = input.trim();
                    match select {
                        "1" => cli.add_a_link(None).await?,
                        "2" => cli.del_a_link().await,
                        "3" => cli.add_subgroup_filter().await,
                        "4" => cli.del_subgroup_filter().await,
                        "5" => cli.add_single_magnet_download().await,
                        "6" => cli.download_a_folder().await?,
                        "7" => {
                            println!("正在退出...");
                            break;
                        }
                        _ => continue,
                    }
                }
            }
        }

        Ok(())
    }

    pub async fn add_a_link(&mut self, link: Option<&str>) -> Result<(), io::Error> {
        let mut input = String::new();
        let rss_link = match link {
            Some(link) => link,
            None => {
                println!("请输入要添加的RSS链接:");
                std::io::stdin()
                    .read_line(&mut input)
                    .expect("just assume stdin will never throw error");
                input.trim()
            }
        };
        if rss_link.is_empty() {
            println!("RSS链接不能为空");
            return Ok(());
        }
        self.write_str(ADD_LINK).await?;
        self.write_str(rss_link).await?;
        self.read_str_to_end().await?;
        Ok(())
    }
    pub async fn del_a_link(&mut self) {}
    pub async fn add_subgroup_filter(&mut self) {}
    pub async fn del_subgroup_filter(&mut self) {}
    pub async fn add_single_magnet_download(&mut self) {}
    pub async fn download_a_folder(&mut self) -> Result<(), io::Error> {
        let mut input = String::new();
        println!("请输入要下载的文件夹cid:");
        std::io::stdin().read_line(&mut input)?;
        let cid = input.trim();
        if cid.is_empty() {
            println!("cid不能为空");
            return Ok(());
        }
        self.write_str(DOWNLOAD_FOLDER).await?;
        self.write_str(cid).await?;
        self.read_str_to_end().await?;
        Ok(())
    }

    pub async fn update(&mut self) -> Result<(), io::Error> {
        self.write_str("Update!").await?;
        println!("stream: {}", self.read_str().await?);
        Ok(())
    }
    pub async fn exit() {}
}

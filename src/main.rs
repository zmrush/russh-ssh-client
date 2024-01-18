mod command;
mod ssh;
mod extract_websocket_stream;
mod async_fs_stream;
use std::sync::Arc;
use async_trait::async_trait;
use command::{command_loop, ExecCommands};
use env_logger::Target;
use russh::*;
use russh_keys::*;
use tokio_tungstenite::{connect_async, client_async};
use bytebuffer::ByteBuffer;
use log::info;
use std::io::Write;
#[cfg(feature = "vsock-support")]
use tokio_vsock::VsockStream;

use crossterm::terminal::disable_raw_mode;
use crate::extract_websocket_stream::ExtractWebsocketStream;
use clap::{Parser,Subcommand};

struct Client {}


#[async_trait]
impl client::Handler for Client {
    type Error = russh::Error;

    async fn check_server_key(
        self,
        _: &key::PublicKey,
    ) -> std::result::Result<(Self, bool), Self::Error> {
        Ok((self, true))
    }
}


/// ssh args
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[cfg(feature="vsock-support")]
    /// cid
    #[arg(short, long)]
    cid: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}
#[derive(Subcommand, Debug)]
enum Commands{
    /// exec command
    Exec{
        #[command(subcommand)]
        command: ExecCommands,
    },
    /// ssh to remote
    Ssh{
        /// term
        #[arg(short, long, default_value_t = String::from("xterm-256color"))]
        term: String,
    },
    /// sftp command
    Sftp{
        /// if from remote to local
        #[arg(short, long, default_value_t = false)]
        reverse: bool,
        /// remote location
        #[arg(short, long)]
        remote: String,

        /// local location
        #[arg(short, long)]
        local: String,
    }
}

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>>{
    let mut builder = env_logger::Builder::from_default_env();
    // builder.format_timestamp_micros();
    let builder = builder.format(|buf, record| {
        writeln!(
            buf,
            "[{} {} {}] [{}:{}] - {}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S.3f %z"),
            record.level(),
            record.module_path().unwrap_or("unknown"),
            record.file().unwrap_or("unknown"),
            record.line().unwrap_or(0),
            record.args()
        )
    });
    let home_dir =  std::env::var("HOME").unwrap();
    let target = Box::new(std::fs::File::create(home_dir.clone() + "/russh-ssh-client.log").expect("Can't create file"));
    builder
    .filter_level(log::LevelFilter::Info)
    .target(Target::Pipe(target))
    .init();



    let config = client::Config {
        inactivity_timeout: Some(std::time::Duration::from_secs(5)),
        ..<_>::default()
    };
    let config = Arc::new(config);
    let sh = Client {};
    let key_pair = load_secret_key(home_dir + "/.ssh/id_ed25519", None)?;
    let args = Args::parse();


    // websocket
    //------------------------------------------------------------------------------------------------------------------------
    // let key_pair = load_secret_key("~/.ssh/id_ed25519", None)?;
    // let url = url::Url::parse("ws://127.0.0.1:7777/ops/ssh").unwrap();
    // let (ws_stream, _) = connect_async(url).await.expect("Failed to connect");
    // let wsss = WebsocketStream{websocket:ws_stream,byte_buffer:ByteBuffer::new()};
    //------------------------------------------------------------------------------------------------------------------------

    //websocket - over tcp
    //------------------------------------------------------------------------------------------------------------------------
    #[cfg(not(feature="vsock-support"))]
    let wsss = {
        let tcp_socket = tokio::net::TcpSocket::new_v4().unwrap();
        let tcp_stream = tcp_socket.connect("127.0.0.1:7777".parse().unwrap()).await?;
        let request = "ws://127.0.0.1:1077/ops/ssh";
        let (ws_stream,_) = client_async(request, tcp_stream).await?;
        ExtractWebsocketStream{ websocket: ws_stream, byte_buffer: ByteBuffer::new() }
    };
    //------------------------------------------------------------------------------------------------------------------------

    //tcp
    //------------------------------------------------------------------------------------------------------------------------
    // #[cfg(not(feature="vsock-support"))]
    // let wsss = tokio::net::TcpStream::connect(("127.0.0.1", 22)).await.unwrap();

    //------------------------------------------------------------------------------------------------------------------------

    // websocket - over vsock
    //------------------------------------------------------------------------------------------------------------------------
    #[cfg(feature="vsock-support")]
    let wsss = {
        let cid = u32::from_str_radix(args.cid.as_ref().unwrap(),10)?;
        let vsock_stream = VsockStream::connect(cid, 1027).await?;
        let request = "ws://127.0.0.1:1027/ops/ssh";
        let (ws_stream,_) = client_async(request, vsock_stream).await?;
        ExtractWebsocketStream{ websocket: ws_stream, byte_buffer: ByteBuffer::new() }
    };
    //------------------------------------------------------------------------------------------------------------------------

    let mut session = russh::client::connect_stream(config, wsss, sh).await?;
    let _auth_res = session
    .authenticate_publickey("zhuming", Arc::new(key_pair))
    .await?;
    let channel = session.channel_open_session().await?;


    if let Some(sub_cmd) = args.command{
        match sub_cmd{
            Commands::Exec { command } =>{
                let res = command_loop(
                    #[cfg(feature="vsock-support")]
                    args.cid,
                    command
                ).await;
                if res.is_err(){
                    info!("exec command error:{:?}",res.err())
                }
            },
            Commands::Ssh { term } =>{
                //异常的情况下，我们要额外进行一次disable raw mode
                let ex = ssh::ssh_loop(term.as_str(), channel).await.or_else(|e| {
                    disable_raw_mode()?;
                    Err(e)
                });
                info!("ex:{:?}",ex);
            },
            Commands::Sftp { reverse, remote, local } =>{
                let res = sftp_loop(reverse, remote, local, channel).await;
                info!("sftp res:{:?}",res);
            }
        }
    }
    Ok(())
}
use russh_sftp::client::SftpSession;
use russh::client::Msg;
pub async fn sftp_loop(reverse: bool,remote: String,local: String,mut channel: Channel<Msg>) -> std::result::Result<(),Box<dyn std::error::Error>>{
    if reverse{
        //从远端到近端
        info!("request");
        channel.request_subsystem(true, "sftp").await?;
        info!("session");
        let sftp = SftpSession::new(channel.into_stream()).await?;
        info!("remote file");
        let mut remote_file = sftp.open(remote).await?;
        info!("local file");
        let mut local_file = tokio::fs::OpenOptions::new().create(true).write(true).open(local).await?;
        info!("start to copy");
        tokio::io::copy(&mut remote_file,&mut local_file).await?;
        info!("copy finish");
    }else{
        // read write append truncate create create_new
        //从近端到远端
        info!("request");
        channel.request_subsystem(true, "sftp").await?;
        info!("session");
        let sftp = SftpSession::new(channel.into_stream()).await?;
        info!("local file");
        let mut local_file = tokio::fs::OpenOptions::new().read(true).open(local).await?;
        info!("remote file");
        let mut remote_file = sftp.create(remote).await?;
        info!("start to copy");
        tokio::io::copy(&mut local_file,&mut remote_file).await?;
        info!("copy finish");
    }
    Ok(())
}





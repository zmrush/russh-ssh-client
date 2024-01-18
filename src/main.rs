mod command;
mod ssh;
mod extract_websocket_stream;
mod async_fs_stream;
use std::sync::Arc;
use async_trait::async_trait;
use command::command_loop;
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

    let mut line = String::new();
    let mut stdout = std::io::stdout();
    stdout.write_all(b"command:")?;
    stdout.flush()?;
    let _ = std::io::stdin().read_line(&mut line).unwrap();
    if line.trim() == "true"{
        let _ = command_loop().await;
        return Ok(());
    }

    let config = client::Config {
        inactivity_timeout: Some(std::time::Duration::from_secs(5)),
        ..<_>::default()
    };
    let config = Arc::new(config);
    let sh = Client {};

   
    let term = "xterm-256color";

    let key_pair = load_secret_key(home_dir + "/.ssh/id_ed25519", None)?;

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
    let (key_pair,wsss) = {
        let tcp_socket = tokio::net::TcpSocket::new_v4().unwrap();
        let tcp_stream = tcp_socket.connect("127.0.0.1:7777".parse().unwrap()).await?;
        let request = "ws://127.0.0.1:1077/ops/ssh";
        let (ws_stream,_) = client_async(request, tcp_stream).await?;
        (key_pair, ExtractWebsocketStream{websocket:ws_stream,byte_buffer: ByteBuffer::new()})
    };
    //------------------------------------------------------------------------------------------------------------------------

    //tcp
    //------------------------------------------------------------------------------------------------------------------------
    // #[cfg(not(feature="vsock-support"))]
    // let (key_pair,wsss) = {
    //     let wsss = tokio::net::TcpStream::connect(("127.0.0.1", 22))
    //     .await.unwrap();
    //     (key_pair,wsss)
    // };
    //------------------------------------------------------------------------------------------------------------------------

    // websocket - over vsock
    //------------------------------------------------------------------------------------------------------------------------
    #[cfg(feature="vsock-support")]
    let (key_pair,wsss) = {
        let mut line = String::new();
        let mut stdout = std::io::stdout();
        stdout.write_all(b"cid:")?;
        stdout.flush()?;
        let _ = std::io::stdin().read_line(&mut line)?;
        let cid = u32::from_str_radix(line.trim(),10)?;
        println!("cid:{}",cid);
        stdout.write_all(b"term:")?;
        stdout.flush()?;
        line = String::new();
        let _ = std::io::stdin().read_line(&mut line)?;
        let term = line.trim();
        println!("term:{}",term);
        let vsock_stream = VsockStream::connect(cid, 1027).await?;
        let request = "ws://127.0.0.1:1027/ops/ssh";
        let (ws_stream,_) = client_async(request, vsock_stream).await?;
        (key_pair,WebsocketStream{websocket:ws_stream,byte_buffer:ByteBuffer::new()})
    };
    //------------------------------------------------------------------------------------------------------------------------

    let mut session = russh::client::connect_stream(config, wsss, sh).await?;
    let _auth_res = session
    .authenticate_publickey("zhuming", Arc::new(key_pair))
    .await?;
    let mut channel = session.channel_open_session().await?;

    let mut line = String::new();
    let mut stdout = std::io::stdout();
    stdout.write_all(b"sftp:")?;
    stdout.flush()?;
    let stdin = std::io::stdin();
    let _ = stdin.read_line(&mut line).unwrap();
    if line.trim() == "true"{
        use russh_sftp::client::SftpSession;
        line = String::new();
        let _ = stdin.read_line(&mut line).unwrap();
        let mut split = line.trim().split_whitespace();
        match (split.next(),split.next(),split.next()){
            (Some(_),Some(remote),Some(local)) =>{
                //从远端到近端
                channel.request_subsystem(true, "sftp").await?;
                let sftp = SftpSession::new(channel.into_stream()).await?;
                let mut remote_file = sftp.open(remote).await?;
                let mut local_file = tokio::fs::OpenOptions::new().create(true).write(true).open(local).await?;
                tokio::io::copy(&mut remote_file,&mut local_file).await?;
                info!("copy finish");
                return Ok(());
            }
            (Some(local),Some(remote),None) =>{
                // read write append truncate create create_new
                //从近端到远端
                channel.request_subsystem(true, "sftp").await?;
                let sftp = SftpSession::new(channel.into_stream()).await?;
                let mut local_file = tokio::fs::OpenOptions::new().read(true).open(local).await?;
                let mut remote_file = sftp.create(remote).await?;
                tokio::io::copy(&mut local_file,&mut remote_file).await?;
                info!("copy finish");
                return Ok(());
            }
            _ =>{
                info!("dont know what to do");
                return Ok(());
            }
        }

    }
    //异常的情况下，我们要额外进行一次disable raw mode
    ssh::ssh_loop(term, channel).await.or_else(|e| {
        disable_raw_mode()?;
        Err(e)
    })?;
    Ok(())
}





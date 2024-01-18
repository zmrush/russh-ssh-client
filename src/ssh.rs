use russh::{Channel, client::Msg};
use crossterm::terminal::window_size;
use std::os::fd::IntoRawFd;
use tokio::io::AsyncReadExt;
use signal_hook::consts::signal::*;
use signal_hook_tokio::Signals;
use futures::stream::StreamExt;
use tokio::io::AsyncWriteExt;
use crossterm::terminal::{enable_raw_mode,disable_raw_mode};

use crate::async_fs_stream::AsyncFsStream;

pub async fn ssh_loop(term: &str,mut channel: Channel<Msg>,) -> std::result::Result<(),Box<dyn std::error::Error>>{

    enable_raw_mode()?;
    let win_size = window_size()?;
    let modes = [];
    let _ = channel.request_pty(true, term, win_size.columns as u32, win_size.rows as u32, 0 , 0 , &modes).await;

    // let stream = channel.into_stream();
    // let (mut stream_reader, mut stream_writer) = tokio::io::split(stream);
    //--------------------------------------------------------------------------------------------------------------------------------------------------------
    let (fd, _) = if unsafe { libc::isatty(libc::STDIN_FILENO) == 1 } {
        (libc::STDIN_FILENO, false)
    } else {
        (
            std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .open("/dev/tty")?
                .into_raw_fd(),
            true,
        )
    };

    let mut signals = Signals::new(&[
        SIGWINCH
    ])?;
    let handle = signals.handle();
    // handle.close();

    let mut raw = AsyncFsStream::new(fd,false).unwrap();
    //这里将stdout 提出来非常重要，否则
    let mut stdout = tokio::io::stdout();
    loop{
        let mut buffer = bytes::BytesMut::with_capacity(1024);
        tokio::select! {
            Some(_) = signals.next() =>{
                let win_size = window_size().unwrap();
                channel.window_change(win_size.columns as u32, win_size.rows as u32, 0, 0).await?;
            }
            Ok(len) = raw.read_buf(&mut buffer) =>{
                let d_vec = &buffer[0..len];
                //info!("from console:{}",String::from_utf8_lossy(d_vec).to_owned());
                channel.data(d_vec).await?;
            }
            Some(msg) = channel.wait() =>{
                match msg {
                    russh::ChannelMsg::Data { ref data } => {
                        use log::info;
                        info!("receive from server:{}",String::from_utf8_lossy(&data.to_vec()).to_owned());
                        // 这里不能使用 标准的std的stdout，因为会抛出 would block 的 error，因为 怀疑这里是 fifo，采用 block的方式的话，就会出现 would block 的 error
                        // std::io::stdout().write_all(&data.to_vec())?;
                        // std::io::stdout().flush()?;
                        // 如果每次都是用 stdout(),会用新的stdou，导致下面的flush不会对上面的write_all进行flush，会出现已经收到bytes，但是屏幕上不会显示 bytes的问题
                        // tokio::io::stdout().write_all(&data.to_vec()).await?;
                        // tokio::io::stdout().flush().await?;
                        stdout.write_all(&data.to_vec()).await?;
                        stdout.flush().await?;
                        
                    }
                    russh::ChannelMsg::ExitStatus { .. } => {
                        channel.close().await?;
                        break;
                    }
                    russh::ChannelMsg::Eof =>{
                        channel.close().await?;
                        break;
                    }
                    _ => {}
                }
            }
        }
    }   
    handle.close();
    disable_raw_mode()?;
    return Ok(());
}
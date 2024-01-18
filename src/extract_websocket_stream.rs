use tokio::io::{AsyncRead,AsyncWrite};
use bytebuffer::ByteBuffer;
use futures::ready;
use std::{
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::ReadBuf;
use log::{info,warn};
use futures_util::StreamExt;
use futures_util::Sink;
pub struct ExtractWebsocketStream<S>{
    pub websocket: tokio_tungstenite::WebSocketStream<S>,
    pub byte_buffer: ByteBuffer
}

impl<S> AsyncRead for ExtractWebsocketStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin
    {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>>{
        if !self.byte_buffer.is_empty(){
            let next_len = buf.remaining().min(self.byte_buffer.len()-self.byte_buffer.get_rpos());
            let rrr = self.byte_buffer.read_bytes(next_len).unwrap();
            if self.byte_buffer.get_rpos() == self.byte_buffer.len(){
                self.byte_buffer.clear();
            }
            buf.put_slice(&rrr);
            return Poll::Ready(Ok(()));
        }

        match ready!(self.websocket.poll_next_unpin(cx)){
            Some(Ok(item)) =>{
                if item.is_binary(){
                    let bin = item.into_data();
                    if buf.remaining()<bin.len(){
                        let now_read = buf.remaining().min(bin.len());
                        buf.put_slice(&bin[..now_read]);
                        self.byte_buffer.write_bytes(&bin[now_read..]);
                    }else{
                        buf.put_slice(&bin);
                    }
                    return Poll::Ready(Ok(()));
                }
                return Poll::Pending;
            }
            Some(Err(e)) =>{
                warn!("read error:{:?}",e);
                return Poll::Ready(Err(std::io::Error::last_os_error()));
            }   
            None =>{
                info!("websocket closed");
                Poll::Ready(Ok(()))
            }
        }
    }
}

impl<S> AsyncWrite for ExtractWebsocketStream<S>
where
    S: AsyncRead + AsyncWrite + Unpin{
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::result::Result<usize, std::io::Error>>{
        match ready!(Pin::new(&mut self.websocket).poll_ready(cx)){
            Ok(()) =>{
                let msg = tokio_tungstenite::tungstenite::Message::Binary(buf.to_vec());
                match Pin::new(&mut self.websocket).start_send(msg){
                    Ok(()) =>{
                        let len = buf.len();
                        return Poll::Ready(Ok(len));
                    }
                    Err(e) =>{
                        info!("send fail:{:?}",e);
                        return Poll::Ready(Err(std::io::Error::last_os_error()))
                    }
                }
            }
            Err(e) =>{
                info!("write ready error:{:?}",e);
                Poll::Ready(Err(std::io::Error::last_os_error()))
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::result::Result<(), std::io::Error>>{
        match ready!(Pin::new(&mut self.websocket).poll_flush(cx)){
            Ok(()) =>{
                Poll::Ready(Ok(()))
            }
            Err(e) =>{
                info!("flush fail:{:?}",e);
                Poll::Ready(Err(std::io::Error::last_os_error()))
            }
        }
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::result::Result<(), std::io::Error>>{
        match ready!(Pin::new(&mut self.websocket).poll_close(cx)){
            Ok(()) =>{
                Poll::Ready(Ok(()))
            }
            Err(e) =>{
                info!("shutdown error:{:?}",e);
                Poll::Ready(Err(std::io::Error::last_os_error()))
            }
        }
    }
}
use std::io::Write;
use futures_util::StreamExt;
use hyper::{Method, Request};
use hyper::Client;
use serde::{Serialize, Deserialize};


#[derive(Serialize, Deserialize)]
pub struct RunCommandRequest{
    pub command_id: String,
    pub command: Vec<u8>,
    pub timeout: u32,
    pub kill_mode: bool,
    pub client_token: String
}
#[derive(Serialize, Deserialize)]
pub struct StopCommandRequest{
    pub command_id: String
}
#[derive(Serialize, Deserialize)]
pub struct DescribeCommandRequest{
    pub command_id: String,
    pub output: bool
}



pub async fn command_loop() -> std::result::Result<(),Box<dyn std::error::Error>>{

    //------------------------------------------------------------------------------------------------------------------------
    // over tcp
    #[cfg(not(feature = "vsock-support"))]
    let client = Client::new();
    #[cfg(not(feature = "vsock-support"))]
    let uuu = "http://localhost:7777".to_string();
    // let tcp_socket = tokio::net::TcpSocket::new_v4().unwrap();
    // let stream = tcp_socket.connect("127.0.0.1:7777".parse().unwrap()).await.unwrap();
    //------------------------------------------------------------------------------------------------------------------------
    #[cfg(feature = "vsock-support")]
    let client = {
        use tokio_vsock::VsockStream;
        use hyper::Uri;
        use futures::future::BoxFuture;
        use std::{
            pin::Pin,
            task::{Context, Poll},
        };
        use tokio::io::{ReadBuf,AsyncRead,AsyncWrite};
        #[derive(Clone)]
        pub struct VsockConnector{
        }
        pub struct VsockStreamConnection{
            pub stream: VsockStream
        }
        impl VsockConnector{
            pub fn new() ->Self{
                Self{}
            }
        }
        impl tower_service::Service<Uri> for VsockConnector{
            type Response = VsockStreamConnection;
            type Error = std::io::Error;
            type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;
            fn call(&mut self, req: Uri) -> Self::Future {

                let fut = async move{
                    let cid = u32::from_str_radix(req.host().unwrap(),10).unwrap();
                    println!("cid:{}",cid);
                    let stream = VsockStream::connect(cid, req.port_u16().unwrap() as u32).await?;
                    Ok(VsockStreamConnection{stream})
                };
                Box::pin(fut)
            }
            fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
                Poll::Ready(Ok(()))
            }
        }
        impl hyper::client::connect::Connection for VsockStreamConnection {
            fn connected(&self) -> hyper::client::connect::Connected {
                hyper::client::connect::Connected::new()
            }
        }
        impl AsyncRead for VsockStreamConnection{
            fn poll_read(
                mut self: Pin<&mut Self>,
                cx: &mut Context<'_>,
                buf: &mut ReadBuf<'_>,
            ) -> Poll<std::io::Result<()>>{
                Pin::new(&mut self.stream).poll_read(cx, buf)
            }
        }
        impl AsyncWrite for VsockStreamConnection{
            fn poll_write(
                mut self: Pin<&mut Self>,
                cx: &mut Context<'_>,
                buf: &[u8],
            ) -> Poll<std::result::Result<usize, std::io::Error>>{
                Pin::new(&mut self.stream).poll_write(cx, buf)
            }
            fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::result::Result<(), std::io::Error>>{
                Pin::new(&mut self.stream).poll_flush(cx)
            }
            fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::result::Result<(), std::io::Error>>{
                Pin::new(&mut self.stream).poll_shutdown(cx)
            }
        }
        Client::builder().build::<VsockConnector,hyper::Body>(VsockConnector::new())
    };
    #[cfg(feature = "vsock-support")]
    let uuu = 
    {
        let mut line = String::new();
        let mut stdout = std::io::stdout();
        stdout.write_all(b"cid:")?;
        stdout.flush()?;
        let _ = std::io::stdin().read_line(&mut line).unwrap();
        let cid = line.trim();
        println!("cid:{}",cid);
        let mut uuu = "vsock://".to_string();
        uuu.push_str(cid);
        uuu.push_str(":1027");
        stdout.write_all(uuu.as_bytes())?;
        stdout.flush()?;
        uuu
    };
    
    //------------------------------------------------------------------------------------------------------------------------
    loop{
        let mut line = String::new();
        let mut stdout = std::io::stdout();
        stdout.write_all(b"type:")?;
        stdout.flush()?;
        let _ = std::io::stdin().read_line(&mut line).unwrap();
        let line = line.trim();
        if line == "run"{

            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line).unwrap();
            let command_id = line.trim();

            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line).unwrap();
            let command = line.trim();

            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line).unwrap();
            let timeout = u32::from_str_radix(line.trim(),10).unwrap();

            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line).unwrap();
            let kill_mode = line.trim() == "true";

            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line).unwrap();
            let client_token = line.trim();

            let request = RunCommandRequest{
                command_id: command_id.to_string(),
                command: command.as_bytes().to_vec(),
                timeout: timeout,
                kill_mode: kill_mode,
                client_token: client_token.to_string()
            };
            let req = Request::builder()
                .method(Method::POST)
                .header("content-type" ,"application/json")
                .uri(uuu.clone() + "/ops/run_command")
                .body(serde_json::to_vec(&request).unwrap().into())
                .expect("request builder");
            let mut resp = client.request(req).await?;
            if resp.status().is_success(){
                if let Some(Ok(body)) = resp.body_mut().next().await{
                    println!("{}",String::from_utf8_lossy(&body.to_vec()).into_owned());
                }
            }
        }else if line == "stop"{
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line).unwrap();
            let command_id = line.trim();

            let request = StopCommandRequest{
                command_id: command_id.to_string()
            };
            let req = Request::builder()
                .method(Method::POST)
                .header("content-type" ,"application/json")
                .uri(uuu.clone() +"/ops/stop_command")
                .body(serde_json::to_vec(&request).unwrap().into())
                .expect("request builder");
            let mut resp = client.request(req).await?;
            if resp.status().is_success(){
                if let Some(Ok(body)) = resp.body_mut().next().await{
                    println!("{}",String::from_utf8_lossy(&body.to_vec()).into_owned());
                }
            }
        }else if line == "describe"{
            let mut line = String::new();
            let _ = std::io::stdin().read_line(&mut line).unwrap();
            let command_id = line.trim();

            let request = DescribeCommandRequest{
                command_id: command_id.to_string(),
                output: true
            };
            let req = Request::builder()
                .method(Method::POST)
                .header("content-type" ,"application/json")
                .uri(uuu.clone() +"/ops/describe_command")
                .body(serde_json::to_vec(&request).unwrap().into())
                .expect("request builder");
            let mut resp = client.request(req).await?;
            if resp.status().is_success(){
                if let Some(Ok(body)) = resp.body_mut().next().await{
                    println!("{}",String::from_utf8_lossy(&body.to_vec()).into_owned());
                }
            }
        }else{
            stdout.write_all(b"unrecognized")?;
            stdout.flush()?;
            continue;
        }
    }
}
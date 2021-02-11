use futures::{AsyncReadExt, AsyncWriteExt};
use std::{env, io::Error};
use wae::net::TcpListener;

#[wae::main]
async fn main() -> Result<(), Error> {
    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "localhost:0".to_string());

    let mut listener = TcpListener::bind(addr).await?;
    let addr = listener.local_addr()?;
    println!("listening on {}", addr);

    loop {
        let (mut stream, _) = listener.accept().await?;

        wae::spawn(async move {
            let mut buf = vec![0; 1024];

            loop {
                let n = stream
                    .read(&mut buf)
                    .await
                    .expect("failed to read data from socket");

                if n == 0 {
                    return;
                }

                stream
                    .write_all(&buf[0..n])
                    .await
                    .expect("failed to write data to socket");
            }
        });
    }
}

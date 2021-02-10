use std::net::SocketAddr;

use futures::{AsyncReadExt, AsyncWriteExt, StreamExt, TryStreamExt};
use wae::net::{TcpListener, TcpStream};

type Result = std::io::Result<()>;

#[wae::test]
async fn roundtrip() -> Result {
    let listener = TcpListener::bind(("localhost", 0)).await?;
    let addr = listener.local_addr()?;
    let listener = wae::spawn(server(listener));
    for _ in 0..4 {
        wae::spawn(client(addr)).detach();
    }
    listener.await
}

async fn server(mut listener: TcpListener) -> Result {
    listener
        .incoming()
        .take(4)
        .try_for_each_concurrent(4, |(stream, _)| wae::spawn(handle(stream)))
        .await
}

async fn client(addr: SocketAddr) -> Result {
    let mut stream = TcpStream::connect(addr).await?;
    stream.write_all(b"Hello").await?;
    let mut buf = [0; 5];
    stream.read_exact(&mut buf).await?;
    assert_eq!(&buf, b"World");
    Ok(())
}

async fn handle(mut stream: TcpStream) -> Result {
    let mut buf = [0; 5];
    stream.read_exact(&mut buf).await?;
    assert_eq!(&buf, b"Hello");
    stream.write_all(b"World").await
}

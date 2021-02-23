use futures::{StreamExt, TryStreamExt};
use std::net::SocketAddr;
use wae::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

type Result = std::io::Result<()>;

#[wae::test]
async fn roundtrip() -> Result {
    let listener = TcpListener::bind(("localhost", 0)).await?;
    let addr = listener.local_addr()?;
    let listener = wae::spawn(server(listener));
    for _ in 0..1 {
        wae::spawn(async move { client(addr).await.unwrap() });
    }
    listener.await
}

async fn server(listener: TcpListener) -> Result {
    listener
        .incoming()
        .take(1)
        .try_for_each_concurrent(1, |(stream, _)| wae::spawn(handle(stream)))
        .await
}

async fn client(addr: SocketAddr) -> Result {
    let mut stream = TcpStream::connect(addr).await?;
    stream.write_all(b"Hello".as_ref()).await?;
    let mut buf = [0; 5];
    stream.read_exact(buf.as_mut()).await?;
    assert_eq!(&buf, b"World");
    Ok(())
}

async fn handle(mut stream: TcpStream) -> Result {
    let mut buf = [0; 5];
    stream.read_exact(buf.as_mut()).await?;
    assert_eq!(&buf, b"Hello");
    stream.write_all(b"World".as_ref()).await
}

use hyper::{client::conn::Builder, Body, Request, StatusCode};
use wae::net::TcpStream;

type Result = std::result::Result<(), Box<dyn std::error::Error + Send + Sync + 'static>>;

#[ignore = "requires network"]
#[wae::test]
async fn http() -> Result {
    let target_stream = TcpStream::connect(("example.com", 80)).await?;

    let (mut request_sender, connection) = Builder::new()
        .handshake::<TcpStream, Body>(target_stream)
        .await?;

    wae::spawn(async move { connection.await.ok() }).detach();

    let request = Request::builder()
        .header("Host", "example.com")
        .method("GET")
        .body(Body::from(""))?;

    let response = request_sender.send_request(request).await?;
    assert!(response.status() == StatusCode::OK);

    Ok(())
}

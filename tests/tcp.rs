use wae::net::TcpStream;

type Result = std::io::Result<()>;

#[ignore = "requires network"]
#[wae::test]
async fn connect_v4() -> Result {
    TcpStream::connect(("93.184.216.34", 80)).await?;
    Ok(())
}

#[ignore = "requires network"]
#[wae::test]
async fn connect_v6() -> Result {
    TcpStream::connect(("2606:2800:220:1:248:1893:25c8:1946", 80)).await?;
    Ok(())
}

#[ignore = "requires network"]
#[wae::test]
async fn resolve() -> Result {
    TcpStream::connect(("example.com", 80)).await?;
    TcpStream::connect("example.com:80").await?;
    Ok(())
}

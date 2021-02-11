mod shared;

use hyper::{server::conn::Http, service::service_fn, Body, Request, Response};
use std::{convert::Infallible, env, io::Error};
use wae::net::TcpListener;

use self::shared::hyper::Exec;

#[wae::main]
async fn main() -> Result<(), Error> {
    let addr = env::args()
        .nth(1)
        .unwrap_or_else(|| "localhost:0".to_string());

    let mut listener = TcpListener::bind(addr).await?;
    let addr = listener.local_addr()?;
    println!("listening on {}", addr);

    loop {
        let (stream, _) = listener.accept().await?;

        wae::spawn(async move {
            if let Err(err) = Http::new()
                .with_executor(Exec)
                .serve_connection(stream, service_fn(echo))
                .await
            {
                eprintln!("error while serving HTTP connection: {}", err);
            }
        });
    }
}

async fn echo(req: Request<Body>) -> Result<Response<Body>, Infallible> {
    let content_type = req.headers().get("content-type").cloned();
    let mut response = Response::new(req.into_body());
    if let Some(ct) = content_type {
        response.headers_mut().append("content-type", ct);
    }
    Ok(response)
}

# wae

An async executor based on the Win32 thread pool API

```rust
use futures::channel::oneshot;

#[wae::main]
async fn main() {
    let (tx, rx) = oneshot::channel();

    let hello = wae::spawn(async move {
        let msg = rx.await.unwrap();
        println!("{}", msg);
    });

    tx.send("Hello from wae !").unwrap();
    hello.await;
}
```

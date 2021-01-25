use futures::{
    channel::oneshot,
    stream::{FuturesUnordered, StreamExt},
};
use wae_macros::test;

#[test]
async fn futures_unordered() {
    let stream: FuturesUnordered<_> = (0..10)
        .map(|n| wae::spawn(async move { (n, n + n) }))
        .collect();
    let results: Vec<(i32, i32)> = stream.collect().await;
    for (n, nn) in results {
        assert_eq!(n + n, nn);
    }
}

#[test]
async fn oneshot() {
    let (tx, rx) = oneshot::channel();
    let task = wae::spawn(async move {
        let two = rx.await.unwrap();
        assert_eq!(2, two);
    });
    tx.send(2).unwrap();
    task.await;
}

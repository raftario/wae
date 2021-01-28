use futures::{
    channel::{mpsc, oneshot},
    SinkExt, StreamExt,
};

#[wae::test]
async fn oneshot() {
    let (tx, rx) = oneshot::channel();
    let task = wae::spawn(async move {
        let two = rx.await.unwrap();
        assert_eq!(2, two);
    });
    tx.send(2).unwrap();
    task.await;
}

#[wae::test]
async fn mpsc() {
    let (mut tx1, rx) = mpsc::channel::<i32>(2);
    let mut tx2 = tx1.clone();
    let task1 = wae::spawn(async move {
        tx1.send(1).await.unwrap();
    });
    let task2 = wae::spawn(async move {
        tx2.send(1).await.unwrap();
    });
    task1.await;
    task2.await;
    let two = rx.fold(0, |c, n| async move { c + n }).await;
    assert_eq!(2, two);
}

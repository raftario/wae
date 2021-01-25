use wae::Threadpool;

#[test]
fn block_on() {
    let pool = Threadpool::new().unwrap();
    let two = pool.block_on(async { 1 + 1 }).unwrap();
    assert_eq!(2, two);
}

#[test]
fn spawn() {
    let pool = Threadpool::new().unwrap();
    let two = pool
        .block_on(async { wae::spawn(async { 1 + 1 }).await })
        .unwrap();
    assert_eq!(2, two);
}

#[test]
#[should_panic]
fn propagate_panic() {
    let pool = Threadpool::new().unwrap();
    pool.block_on(async { panic!() }).unwrap();
}

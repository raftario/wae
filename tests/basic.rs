use wae::Threadpool;

#[test]
fn ok() {
    let pool = Threadpool::new().unwrap();
    let two = pool.block_on(async { 1 + 1 });
    assert_eq!(2, two);
}

#[test]
#[should_panic]
fn err() {
    let pool = Threadpool::new().unwrap();
    pool.block_on(async { panic!() });
}

use hyper::rt::Executor;
use std::future::Future;

#[derive(Clone)]
pub struct Exec;
impl<F> Executor<F> for Exec
where
    F: Future + Send + 'static,
    F::Output: Send,
{
    fn execute(&self, fut: F) {
        wae::spawn(fut);
    }
}

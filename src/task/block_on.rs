use std::{
    future::Future,
    task::{Context, Poll},
};

use pin_utils::pin_mut;

use crate::task::waker::InlineWaker;
use crate::threadpool::Handle;

impl Handle {
    pub fn block_on<F, T>(&self, future: F) -> T
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let future = self.spawn(future);
        pin_mut!(future);

        let inline_waker = InlineWaker::default();
        let waker = inline_waker.get_waker();
        let mut cx = Context::from_waker(&waker);

        loop {
            match future.as_mut().poll(&mut cx) {
                Poll::Ready(output) => return output,
                Poll::Pending => inline_waker.wait(),
            }
        }
    }
}

pub fn block_on<F, T>(future: F) -> T
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    Handle::current().block_on(future)
}

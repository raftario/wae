use std::{
    future::Future,
    task::{Context, Poll},
};

use pin_utils::pin_mut;

use crate::task::waker:InlineWaker;
use crate::{
    error::Error,
    threadpool::{Handle, Threadpool},
};

impl Handle {
    pub fn block_on<F, T>(&self, future: F) -> Result<T, Error>
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
                Poll::Ready(output) => return Ok(output),
                Poll::Pending => inline_waker.wait(),
            }
        }
    }
}

impl Threadpool {
    pub fn block_on<F, T>(&self, future: F) -> Result<T, Error>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        self.handle().block_on(future)
    }
}

pub fn block_on<F, T>(future: F) -> Result<T, Error>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    Handle::current().block_on(future)
}

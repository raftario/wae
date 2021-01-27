use std::{
    cell::RefCell,
    future::Future,
    task::{Context, Poll, Waker},
};

use parking::Parker;
use pin_utils::pin_mut;

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

        thread_local! {
            static PARKING: RefCell<(Parker, Waker)> = {
                let (parker, unparker) = parking::pair();
                let waker = waker_fn::waker_fn(move || {
                    unparker.unpark();
                });
                RefCell::new((parker, waker))
            };
        }

        PARKING.with(|cache| {
            let (parker, waker) = &mut *cache
                .try_borrow_mut()
                .map_err(|_| Error::RecursiveBlockOn)?;

            let mut cx = Context::from_waker(&waker);
            loop {
                match future.as_mut().poll(&mut cx) {
                    Poll::Ready(output) => return Ok(output),
                    Poll::Pending => parker.park(),
                }
            }
        })
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

use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    task::{Context, Poll, Waker},
};

use async_task::Task;
use parking::Parker;
use pin_project_lite::pin_project;
use pin_utils::pin_mut;

use winapi::um::threadpoolapiset::{
    CallbackMayRunLong, CreateThreadpoolWork, SubmitThreadpoolWork,
};

use crate::{
    callback::CallbackContext,
    error::Error,
    threadpool::{Handle, Threadpool},
};

pin_project! {
    pub struct JoinHandle<T> {
        #[pin]
        task: Task<T>,
    }
}

impl<T> Future for JoinHandle<T> {
    type Output = T;

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        self.project().task.poll(cx)
    }
}

impl<T> JoinHandle<T> {
    #[inline]
    pub fn detach(self) {
        self.task.detach()
    }

    #[inline]
    pub async fn cancel(self) -> Option<T> {
        self.task.cancel().await
    }
}

impl Handle {
    pub fn spawn<T, F>(&self, future: F) -> JoinHandle<T>
    where
        T: Send + 'static,
        F: Future<Output = T> + Send + 'static,
    {
        let handle = self.clone();
        let schedule = move |runnable| {
            #[cfg(not(feature = "tracing"))]
            let handle = handle.clone();
            #[cfg(feature = "tracing")]
            let mut handle = handle.clone();
            #[cfg(feature = "tracing")]
            {
                handle.span = match handle.span {
                    Some(parent) => Some(tracing::trace_span!(
                        parent: parent,
                        "task",
                        pool = ?handle.callback_environ.Pool
                    )),
                    None => {
                        Some(tracing::trace_span!("task", pool = ?handle.callback_environ.Pool))
                    }
                }
            }

            let mut callback_environ = handle.callback_environ;
            let context = CallbackContext { runnable, handle };
            let context = Box::into_raw(Box::new(context));

            unsafe {
                let work = CreateThreadpoolWork(
                    Some(crate::callback::callback),
                    context as _,
                    &mut callback_environ,
                );
                if work.is_null() {
                    panic!(Error::win32());
                }

                SubmitThreadpoolWork(work);
            }
        };

        let (runnable, task) = async_task::spawn(future, schedule);
        runnable.schedule();

        JoinHandle { task }
    }

    pub fn block_on<T, F>(&self, future: F) -> Result<T, Error>
    where
        T: Send + 'static,
        F: Future<Output = T> + Send + 'static,
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

    pub fn may_block(&self) -> bool {
        match self.callback_instance {
            Some(instance) => unsafe { CallbackMayRunLong(instance) != 0 },
            None => false,
        }
    }
}

impl Threadpool {
    #[inline]
    pub fn spawn<T, F>(&self, future: F) -> JoinHandle<T>
    where
        T: Send + 'static,
        F: Future<Output = T> + Send + 'static,
    {
        self.handle().spawn(future)
    }

    #[inline]
    pub fn block_on<T, F>(&self, future: F) -> Result<T, Error>
    where
        T: Send + 'static,
        F: Future<Output = T> + Send + 'static,
    {
        self.handle().block_on(future)
    }
}

#[inline]
pub fn spawn<T, F>(future: F) -> JoinHandle<T>
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
{
    Handle::current().spawn(future)
}

#[inline]
pub fn block_on<T, F>(future: F) -> Result<T, Error>
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
{
    Handle::current().block_on(future)
}

#[inline]
pub fn may_block() -> bool {
    Handle::current().may_block()
}

#[inline]
pub async fn yield_now() {
    pub struct YieldNow(bool);

    impl Future for YieldNow {
        type Output = ();

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
            if !self.0 {
                self.0 = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            } else {
                Poll::Ready(())
            }
        }
    }

    YieldNow(false).await
}

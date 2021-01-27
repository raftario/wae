use std::{
    ffi::c_void,
    future::Future,
    mem::MaybeUninit,
    panic::{self, AssertUnwindSafe},
    pin::Pin,
    task::{Context, Poll},
};

use async_task_ffi::Runnable;
use pin_project_lite::pin_project;

use winapi::um::{
    threadpoolapiset::{CloseThreadpoolWork, CreateThreadpoolWork, SubmitThreadpoolWork},
    winnt::{PTP_CALLBACK_INSTANCE, PTP_WORK},
};

use crate::{
    error::Error,
    threadpool::{Handle, Threadpool},
};

pin_project! {
    #[must_use = "tasks get canceled when dropped, use `.detach()` to run them in the background"]
    pub struct JoinHandle<T> {
        #[pin]
        task: async_task_ffi::Task<T>,
    }
}

impl<T> Future for JoinHandle<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context) -> Poll<Self::Output> {
        self.project().task.poll(cx)
    }
}

impl<T> JoinHandle<T> {
    pub fn detach(self) {
        self.task.detach()
    }

    pub async fn cancel(self) -> Option<T> {
        self.task.cancel().await
    }
}

struct CallbackContext {
    handle: Handle,
}

unsafe extern "system" fn callback(
    instance: PTP_CALLBACK_INSTANCE,
    context: *mut c_void,
    work: PTP_WORK,
) {
    let runnable: Runnable<MaybeUninit<CallbackContext>> = Runnable::from_raw(context as *mut ());
    let CallbackContext { mut handle } = runnable.data().as_ptr().read();

    handle.set_callback_instance(instance);
    let _context = handle.enter();
    #[cfg(feature = "tracing")]
    let _span = handle.enter_span();

    panic::catch_unwind(AssertUnwindSafe(move || runnable.run())).ok();

    CloseThreadpoolWork(work);
}

impl Handle {
    pub fn spawn<F, T>(&self, future: F) -> JoinHandle<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        #[cfg(not(feature = "tracing"))]
        let handle = self.clone();
        #[cfg(feature = "tracing")]
        let mut handle = self.clone();
        #[cfg(feature = "tracing")]
        {
            handle.span = match handle.span {
                Some(parent) => Some(tracing::trace_span!(
                    parent: parent,
                    "task",
                    pool = ?handle.callback_environ.Pool
                )),
                None => Some(tracing::trace_span!("task", pool = ?handle.callback_environ.Pool)),
            }
        }

        let schedule = move |mut runnable: Runnable<MaybeUninit<CallbackContext>>| {
            let handle = handle.clone();
            let mut callback_environ = handle.callback_environ;

            unsafe {
                let context = CallbackContext { handle };
                runnable.data_mut().as_mut_ptr().write(context);
                let runnable = runnable.into_raw();

                let work = CreateThreadpoolWork(
                    Some(callback),
                    runnable as *mut c_void,
                    &mut callback_environ,
                );
                if work.is_null() {
                    panic!("failed to schedule task: {}", Error::win32());
                }

                SubmitThreadpoolWork(work);
            }
        };

        let (runnable, task) = async_task_ffi::spawn_with(future, schedule, MaybeUninit::uninit());
        runnable.schedule();

        JoinHandle { task }
    }
}

impl Threadpool {
    pub fn spawn<F, T>(&self, future: F) -> JoinHandle<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        self.handle().spawn(future)
    }
}

pub fn spawn<F, T>(future: F) -> JoinHandle<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    Handle::current().spawn(future)
}

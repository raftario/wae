use std::{
    ffi::c_void,
    fmt,
    future::Future,
    mem::{self, ManuallyDrop},
    pin::Pin,
    task::{Context, Poll},
};

use async_task::{Runnable, Task};
use crossbeam_queue::SegQueue;
use pin_utils::pin_mut;

use winapi::um::winnt::{PTP_CALLBACK_INSTANCE, PTP_WORK};

use crate::threadpool::Handle;

pub struct JoinHandle<T> {
    task: ManuallyDrop<Task<T>>,
}

impl<T> Future for JoinHandle<T> {
    type Output = T;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let task = &mut *self.task;
        pin_mut!(task);
        task.poll(cx)
    }
}

impl<T> JoinHandle<T> {
    pub async fn cancel(self) -> Option<T> {
        let mut this = self;
        let output = unsafe { ManuallyDrop::take(&mut this.task) }.cancel().await;
        mem::forget(this);
        output
    }
}

impl<T> Drop for JoinHandle<T> {
    fn drop(&mut self) {
        let task = unsafe { ManuallyDrop::take(&mut self.task) };
        task.detach()
    }
}

impl<T> fmt::Debug for JoinHandle<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JoinHandle")
            .field("task", &*self.task)
            .finish()
    }
}

pub(crate) unsafe extern "system" fn callback(
    instance: PTP_CALLBACK_INSTANCE,
    context: *mut c_void,
    _work: PTP_WORK,
) {
    let context = context as *const SegQueue<(Runnable, Handle)>;
    let queue = &*context;
    let (runnable, mut handle) = queue.pop().unwrap();

    handle.callback_instance.replace(instance);
    let _context = handle.enter();
    #[cfg(feature = "tracing")]
    let _span = handle.enter_span();

    std::panic::catch_unwind(move || runnable.run()).ok();
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
            handle.span = match &handle.span {
                Some(parent) => Some(tracing::trace_span!(
                    parent: parent,
                    "task",
                    handle = ?handle
                )),
                None => Some(tracing::trace_span!("task", handle = ?handle)),
            }
        }

        let schedule = move |runnable| handle.push_task(runnable);

        let (runnable, task) = async_task::spawn(future, schedule);
        runnable.schedule();

        JoinHandle {
            task: ManuallyDrop::new(task),
        }
    }
}

pub fn spawn<F, T>(future: F) -> JoinHandle<T>
where
    F: Future<Output = T> + Send + 'static,
    T: Send + 'static,
{
    Handle::current().spawn(future)
}

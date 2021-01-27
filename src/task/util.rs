use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use winapi::{shared::minwindef::TRUE, um::threadpoolapiset::CallbackMayRunLong};

use crate::threadpool::Handle;

impl Handle {
    pub fn may_block(&self) -> bool {
        match self.callback_instance {
            Some(instance) => unsafe { CallbackMayRunLong(instance) == TRUE },
            None => false,
        }
    }
}

pub fn may_block() -> bool {
    Handle::current().may_block()
}

struct YieldNow(bool);

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

#[inline]
pub async fn yield_now() {
    YieldNow(false).await
}

use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

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

use winapi::{shared::minwindef::TRUE, um::threadpoolapiset::CallbackMayRunLong};

use crate::threadpool::Handle;

impl Handle {
    pub fn may_block(&self) -> bool {
        match self.callback_instance {
            Some(instance) => unsafe { CallbackMayRunLong(instance) == TRUE },
            None => false,
        }
    }

    #[inline]
    pub async fn block_in_place<T, F>(&self, f: F) -> Option<T>
    where
        F: FnOnce() -> T,
    {
        if !self.may_block() {
            return None;
        }

        let output = f();
        super::yield_now().await;
        Some(output)
    }
}

pub fn may_block() -> bool {
    Handle::current().may_block()
}

#[inline]
pub async fn block_in_place<T, F>(f: F) -> Option<T>
where
    F: FnOnce() -> T,
{
    Handle::current().block_in_place(f).await
}

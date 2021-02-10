use std::{cell::RefCell, marker::PhantomData};

use crate::threadpool::Handle;

thread_local! {
    static HANDLE: RefCell<Option<Handle>> = RefCell::new(None);
}

pub struct ContextGuard<'a> {
    previous: Option<Handle>,
    _marker: PhantomData<&'a Handle>,
}
impl Drop for ContextGuard<'_> {
    fn drop(&mut self) {
        HANDLE.with(|h| {
            let mut h = h.borrow_mut();
            *h = self.previous.take();
        })
    }
}

impl Handle {
    pub fn current() -> Handle {
        Self::try_current().expect("no wae context")
    }

    pub fn try_current() -> Option<Handle> {
        HANDLE.with(|h| {
            let h = h.borrow();
            h.clone()
        })
    }

    pub fn enter(&self) -> ContextGuard {
        HANDLE.with(|h| {
            let mut h = h.borrow_mut();
            let previous = h.replace(self.clone());
            ContextGuard {
                previous,
                _marker: PhantomData::default(),
            }
        })
    }

    #[cfg(feature = "tracing")]
    pub(crate) fn enter_span(&self) -> Option<tracing::span::Entered> {
        self.span.as_ref().map(|span| span.enter())
    }
}

pub fn current() -> Handle {
    Handle::current()
}

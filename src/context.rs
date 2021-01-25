use std::{cell::RefCell, marker::PhantomData};

use crate::{error::Error, threadpool::Handle};

thread_local! {
    static HANDLE: RefCell<Option<Handle>> = RefCell::new(None);
}

pub struct ContextGuard<'a> {
    previous: Option<Handle>,
    _marker: PhantomData<&'a Handle>,
    #[cfg(feature = "tracing")]
    _span: Option<tracing::span::Entered<'a>>,
}
impl Drop for ContextGuard<'_> {
    fn drop(&mut self) {
        HANDLE.with(|h| {
            let mut h = h
                .try_borrow_mut()
                .map_err(|_| Error::Unexpected("data race leaving context"))
                .unwrap();
            *h = self.previous.take();
        })
    }
}

impl Handle {
    #[inline]
    pub fn current() -> Self {
        Self::try_current().unwrap()
    }

    pub fn try_current() -> Result<Self, Error> {
        HANDLE.with(|h| {
            let h = h
                .try_borrow()
                .map_err(|_| Error::Unexpected("data race entering context"))?;
            h.clone().ok_or(Error::NoContext)
        })
    }

    #[inline]
    pub fn enter(&self) -> ContextGuard {
        self.try_enter().unwrap()
    }

    pub fn try_enter(&self) -> Result<ContextGuard, Error> {
        HANDLE.with(|h| {
            let mut h = h.try_borrow_mut().map_err(|_| Error::RecursiveContext)?;
            let previous = h.replace(self.clone());
            Ok(ContextGuard {
                previous,
                _marker: PhantomData::default(),
                #[cfg(feature = "tracing")]
                _span: self.span.as_ref().map(|s| s.enter()),
            })
        })
    }
}

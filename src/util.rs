#[cfg(any(feature = "net"))]
pub(crate) trait HeapAllocated<T> {
    fn new(val: T) -> Self;
    fn inner_ptr(&self) -> *const T;
}
#[cfg(any(feature = "net"))]
impl<T> HeapAllocated<T> for Box<T> {
    fn new(val: T) -> Self {
        Box::new(val)
    }

    fn inner_ptr(&self) -> *const T {
        &**self as *const T
    }
}
#[cfg(any(feature = "net"))]
impl<T> HeapAllocated<T> for std::sync::Arc<T> {
    fn new(val: T) -> Self {
        std::sync::Arc::new(val)
    }

    fn inner_ptr(&self) -> *const T {
        std::sync::Arc::as_ptr(self)
    }
}

#[cfg(any(feature = "net"))]
pub(crate) trait Extract {
    type Inner;
}
#[cfg(any(feature = "net"))]
impl<T> Extract for Option<T> {
    type Inner = T;
}

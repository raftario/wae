#[cfg(all(feature = "parking_lot", any(feature = "net")))]
pub(crate) use parking_lot::Mutex;
#[cfg(all(not(feature = "parking_lot"), any(feature = "net")))]
pub(crate) struct Mutex<T>(std::sync::Mutex<T>);
#[cfg(all(not(feature = "parking_lot"), any(feature = "net")))]
impl<T> Mutex<T> {
    pub(crate) fn new(t: T) -> Self {
        Self(std::sync::Mutex::new(t))
    }

    pub(crate) fn lock(&self) -> std::sync::MutexGuard<T> {
        self.0.lock().unwrap()
    }
}

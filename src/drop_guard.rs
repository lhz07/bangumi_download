use std::mem::ManuallyDrop;

pub struct DropGuard<F: FnOnce(T), T> {
    f: ManuallyDrop<F>,
    inner: ManuallyDrop<T>,
}

impl<F: FnOnce(T), T> DropGuard<F, T> {
    pub fn new(inner: T, f: F) -> Self {
        Self {
            f: ManuallyDrop::new(f),
            inner: ManuallyDrop::new(inner),
        }
    }
    pub fn inner(&self) -> &T {
        &self.inner
    }
    pub fn into_inner(self) -> T {
        let mut new_guard = ManuallyDrop::new(self);
        let value = unsafe { ManuallyDrop::take(&mut new_guard.inner) };
        unsafe { ManuallyDrop::drop(&mut new_guard.f) };
        value
    }
}

impl<F: FnOnce(T), T> Drop for DropGuard<F, T> {
    fn drop(&mut self) {
        let value = unsafe { ManuallyDrop::take(&mut self.inner) };
        let f = unsafe { ManuallyDrop::take(&mut self.f) };
        f(value);
    }
}

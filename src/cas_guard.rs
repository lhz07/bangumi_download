use std::cell::UnsafeCell;
use std::ops::{Deref, DerefMut};
use std::panic::{RefUnwindSafe, UnwindSafe};
use std::sync::atomic::AtomicU8;

pub struct CASGuard<T> {
    data: UnsafeCell<T>,
    flag: AtomicU8,
}

const STOPPED: u8 = 0;
const WORKING: u8 = 1;

unsafe impl<T> Send for CASGuard<T> {}
unsafe impl<T> Sync for CASGuard<T> {}
impl<T> UnwindSafe for CASGuard<T> {}
impl<T> RefUnwindSafe for CASGuard<T> {}

struct CASDropGuard<'a> {
    flag: &'a AtomicU8,
}
impl<'a> Drop for CASDropGuard<'a> {
    fn drop(&mut self) {
        self.flag
            .store(STOPPED, std::sync::atomic::Ordering::Release);
    }
}

pub struct CASLockGuard<'a, T> {
    data: &'a mut T,
    _guard: CASDropGuard<'a>,
}

impl<'a, T> Deref for CASLockGuard<'a, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        self.data
    }
}

impl<'a, T> DerefMut for CASLockGuard<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.data
    }
}

impl<T> CASGuard<T> {
    pub const fn new(data: T) -> Self {
        Self {
            data: UnsafeCell::new(data),
            flag: AtomicU8::new(STOPPED),
        }
    }
    pub fn try_to_do<F>(&self, f: F) -> bool
    where
        F: FnOnce(&mut T),
    {
        match self.flag.compare_exchange(
            STOPPED,
            WORKING,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Relaxed,
        ) {
            Ok(_) => {
                let _drop_guard = CASDropGuard { flag: &self.flag };
                // SAFETY: we ensure that there is only one mutable ref
                let data = unsafe { &mut *self.data.get() };
                f(data);
                true
            }
            Err(_) => false,
        }
    }
    pub fn lock<'a>(&'a self) -> Option<CASLockGuard<'a, T>> {
        match self.flag.compare_exchange(
            STOPPED,
            WORKING,
            std::sync::atomic::Ordering::AcqRel,
            std::sync::atomic::Ordering::Relaxed,
        ) {
            Ok(_) => {
                let drop_guard = CASDropGuard { flag: &self.flag };
                // SAFETY: we ensure that there is only one mutable ref
                let data = unsafe { &mut *self.data.get() };
                let lock_guard = CASLockGuard {
                    data,
                    _guard: drop_guard,
                };
                Some(lock_guard)
            }
            Err(_) => None,
        }
    }
}

/// # Tests
/// test with `cargo +nightly miri test cas_guard::tests`
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[test]
    fn basic_test() {
        let guard = CASGuard::new(0usize);
        assert!(guard.try_to_do(|v| *v += 1));
        assert!(guard.try_to_do(|v| *v += 1));
        assert_eq!(*guard.lock().unwrap(), 2);
        {
            let mut g = guard.lock().unwrap();
            *g += 1;
        }
        assert_eq!(*guard.lock().unwrap(), 3);
    }

    #[test]
    fn miri_test() {
        let guard = Arc::new(CASGuard::new(0usize));
        let mut handle = Vec::with_capacity(5);
        for i in 0..5 {
            let g = guard.clone();
            handle.push(std::thread::spawn(move || {
                println!("thread {} start", i);
                for _ in 0..1000 {
                    g.try_to_do(|v| *v += 1);
                    let guard = g.lock();
                    if let Some(mut g) = guard {
                        *g += 1;
                    }
                }
                println!("thread {} finished", i);
            }));
        }
        for h in handle {
            h.join().unwrap();
        }
    }

    #[test]
    fn concurrent_test() {
        let guard = Arc::new(CASGuard::new(0usize));
        let success_count = Arc::new(AtomicUsize::new(0));

        let g = guard.clone();
        let counter = success_count.clone();
        let first_h = std::thread::spawn(move || {
            if g.try_to_do(|v| {
                println!("waiting...");
                std::thread::sleep(Duration::from_millis(500));
                println!("waiting finished!");
                *v += 1;
            }) {
                counter.fetch_add(1, Ordering::Relaxed);
            }
        });
        std::thread::sleep(Duration::from_millis(100));
        println!("spawn");
        let mut handles = Vec::with_capacity(9);
        for _ in 0..9 {
            let g = guard.clone();
            let counter = success_count.clone();
            handles.push(std::thread::spawn(move || {
                if g.try_to_do(|v| {
                    *v += 1;
                }) {
                    counter.fetch_add(1, Ordering::Relaxed);
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }
        first_h.join().unwrap();

        // only one thread can succeed
        assert_eq!(success_count.load(Ordering::Acquire), 1);
        // SAFETY: we ensure that there is only one mutable ref
        let data_val = *guard.lock().unwrap();
        assert_eq!(data_val, 1);
    }

    #[test]
    fn panic_safety_test() {
        let guard = Arc::new(CASGuard::new(0usize));
        let g = guard.clone();
        let _ = std::panic::catch_unwind(|| {
            g.try_to_do(|_| panic!("oops"));
        });
        let _ = std::panic::catch_unwind(|| {
            let mut a = g.lock().unwrap();
            *a += 1;
            panic!("oops");
        });

        // we have ensured the panic safety
        assert!(guard.try_to_do(|v| *v += 1));
        let val = unsafe { *guard.data.get() };
        assert_eq!(val, 2);
    }
}

use core::{
    cell::UnsafeCell,
    sync::atomic::AtomicBool,
    sync::atomic::Ordering::*,
    future::poll_fn,
    task::Poll,
    ops::{Deref, DerefMut},
    };

pub struct BusyMutex<T> {
    value: UnsafeCell<T>,
    locked: AtomicBool,
}
impl<T> BusyMutex<T> {
    pub fn new(value: T) -> Self {
        Self {
            value: value.into(), 
            locked: AtomicBool::new(false),
        }
    }
    pub fn try_lock(&self) -> Option<BusyMutexGuard<'_, T>> {
        BusyMutexGuard::try_new(self)
    }
    /// busy polling future until lock is acquired
    pub async fn lock(&self) -> BusyMutexGuard<'_, T> {
        poll_fn(|_| match BusyMutexGuard::try_new(self) {
            Some(guard) => Poll::Ready(guard),
            None => Poll::Pending,
            }).await
    }
    /// busy wait until lock is acquired
    pub fn blocking_lock(&self) -> BusyMutexGuard<'_, T> {
        loop {
            if let Some(pending) = BusyMutexGuard::try_new(self) 
                {break pending}
            // nothing else to do, leave resources to the kernel
            std::thread::yield_now();
        }
    }
}

pub struct BusyMutexGuard<'m, T> {
    mutex: &'m BusyMutex<T>,
}
impl<'m, T> BusyMutexGuard<'m, T> {
    fn try_new(mutex: &'m BusyMutex<T>) -> Option<Self> {
        if mutex.locked.swap(true, Acquire)
            {Some(Self {mutex})}
        else 
            {None}
    }
}
impl<T> Deref for BusyMutexGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe {& *self.mutex.value.get()}
    }
}
impl<T> DerefMut for BusyMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        unsafe {&mut *self.mutex.value.get()}
    }
}
impl<T> Drop for BusyMutexGuard<'_, T> {
    fn drop(&mut self) {
        self.mutex.locked.store(false, Release);
    }
}

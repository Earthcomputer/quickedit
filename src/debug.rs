use std::ops::{Deref, DerefMut};
use std::sync::{LockResult, Mutex, MutexGuard, PoisonError, TryLockError, TryLockResult};
#[cfg(feature = "profile-with-tracy")]
use tracy_client_sys::*;

#[cfg(feature = "profile-with-tracy")]
pub struct ProfileMutex<T>(Mutex<T>, u32);
#[cfg(not(feature = "profile-with-tracy"))]
pub struct ProfileMutex<T>(Mutex<T>);

#[cfg(feature = "profile-with-tracy")]
pub struct LocDataWrapper(pub tracy_client_sys::___tracy_source_location_data);
#[cfg(feature = "profile-with-tracy")]
unsafe impl Sync for LocDataWrapper {}
#[cfg(feature = "profile-with-tracy")]
unsafe impl Send for LocDataWrapper {}

#[cfg(feature = "profile-with-tracy")]
#[macro_export]
macro_rules! profile_mutex {
    ($name:literal, $value:expr) => {
        {
            struct S;
            static SRC_LOC: std::lazy::SyncLazy<crate::debug::LocDataWrapper> = std::lazy::SyncLazy::new(|| {
                let function_name = std::ffi::CString::new(std::any::type_name::<S>().strip_suffix("::S").unwrap()).unwrap();
                crate::debug::LocDataWrapper(tracy_client_sys::___tracy_source_location_data {
                    name: concat!($name, "\0").as_ptr() as *const _,
                    function: function_name.as_ptr() as *const _,
                    file: concat!(file!(), "\0").as_ptr() as *const _,
                    line: line!(),
                    color: 0,
                })
            });
            crate::debug::ProfileMutex::new($value, &SRC_LOC.0)
        }
    }
}

#[cfg(not(feature = "profile-with-tracy"))]
#[macro_export]
macro_rules! profile_mutex {
    ($name:literal, $value:expr) => {
        $crate::debug::ProfileMutex::new($value)
    }
}

#[cfg(feature = "profile-with-tracy")]
impl<T> ProfileMutex<T> {
    pub fn new(value: T, srcloc: &'static ___tracy_source_location_data) -> Self {
        unsafe {
            ___tracy_init_thread();
            let id = __tracy_alloc_lockable_ctx(srcloc);
            Self(Mutex::new(value), id)
        }
    }

    pub fn lock(&self) -> LockResult<ProfileMutexGuard<'_, T>> {
        unsafe {
            ___tracy_init_thread();
            let run_after = __tracy_lockable_ctx_before_lock(self.1);
            let guard = self.0.lock();
            if run_after { __tracy_lockable_ctx_after_lock(self.1); }
            guard.map(|g| ProfileMutexGuard(Some(g), self.1))
                .map_err(|err| PoisonError::new(ProfileMutexGuard(Some(err.into_inner()), self.1)))
        }
    }

    pub fn try_lock(&self) -> TryLockResult<ProfileMutexGuard<'_, T>> {
        unsafe {
            ___tracy_init_thread();
            let guard = self.0.try_lock();
            let acquired = guard.is_ok();
            __tracy_lockable_ctx_after_try_lock(self.1, acquired);
            guard.map(|g| ProfileMutexGuard(Some(g), self.1))
                .map_err(|err| match err {
                    TryLockError::Poisoned(err) => TryLockError::Poisoned(PoisonError::new(ProfileMutexGuard(Some(err.into_inner()), self.1))),
                    TryLockError::WouldBlock => TryLockError::WouldBlock,
                })
        }
    }

    pub fn mark(&self, srcloc: &'static ___tracy_source_location_data) {
        unsafe {
            __tracy_lockable_ctx_mark(self.1, srcloc);
        }
    }

    pub fn custom_name(&self, name: &str) {
        unsafe {
            __tracy_lockable_ctx_custom_name(self.1, name.as_ptr() as _, name.len());
        }
    }
}

#[cfg(not(feature = "profile-with-tracy"))]
impl<T> ProfileMutex<T> {
    #[inline]
    pub fn new(value: T) -> Self {
        Self(Mutex::new(value))
    }

    pub fn lock(&self) -> LockResult<ProfileMutexGuard<'_, T>> {
        self.0.lock().map(|g| ProfileMutexGuard(g))
            .map_err(|err| PoisonError::new(ProfileMutexGuard(err.into_inner())))
    }

    pub fn try_lock(&self) -> TryLockResult<ProfileMutexGuard<'_, T>> {
        self.0.try_lock().map(|g| ProfileMutexGuard(g))
            .map_err(|err| match err {
                TryLockError::Poisoned(err) => TryLockError::Poisoned(PoisonError::new(ProfileMutexGuard(err.into_inner()))),
                TryLockError::WouldBlock => TryLockError::WouldBlock,
            })
    }
}

#[cfg(feature = "profile-with-tracy")]
impl<T> Drop for ProfileMutex<T> {
    fn drop(&mut self) {
        unsafe {
            __tracy_dealloc_lockable_ctx(self.1);
        }
    }
}

#[cfg(feature = "profile-with-tracy")]
pub struct ProfileMutexGuard<'a, T>(Option<MutexGuard<'a, T>>, u32);
#[cfg(not(feature = "profile-with-tracy"))]
pub struct ProfileMutexGuard<'a, T>(MutexGuard<'a, T>);

#[cfg(feature = "profile-with-tracy")]
impl<T> Drop for ProfileMutexGuard<'_, T> {
    fn drop(&mut self) {
        let guard = self.0.take();
        drop(guard);
        unsafe {
            __tracy_lockable_ctx_after_unlock(self.1);
        }
    }
}

impl<'a, T> Deref for ProfileMutexGuard<'a, T> {
    type Target = T;

    #[cfg(feature = "profile-with-tracy")]
    #[inline]
    fn deref(&self) -> &T {
        &*self.0.as_ref().unwrap()
    }

    #[cfg(not(feature = "profile-with-tracy"))]
    #[inline]
    fn deref(&self) -> &T {
        &self.0
    }
}

impl<'a, T> DerefMut for ProfileMutexGuard<'a, T> {
    #[cfg(feature = "profile-with-tracy")]
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        &mut *self.0.as_mut().unwrap()
    }

    #[cfg(not(feature = "profile-with-tracy"))]
    #[inline]
    fn deref_mut(&mut self) -> &mut T {
        &mut self.0
    }
}

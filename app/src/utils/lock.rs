use std::sync::{RwLock, RwLockReadGuard};

pub fn read_or_recover<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|poisoned| {
        log::error!("read lock was poisoned; recovering the last committed state");
        poisoned.into_inner()
    })
}

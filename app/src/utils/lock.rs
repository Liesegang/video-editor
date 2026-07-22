use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

pub fn mutex_lock_or_recover<T>(lock: &Mutex<T>) -> MutexGuard<'_, T> {
    lock.lock().unwrap_or_else(|poisoned| {
        log::error!("mutex was poisoned; recovering its last committed state");
        poisoned.into_inner()
    })
}

pub fn read_or_recover<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    lock.read().unwrap_or_else(|poisoned| {
        log::error!("read lock was poisoned; recovering the last committed state");
        poisoned.into_inner()
    })
}

pub fn write_or_recover<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    lock.write().unwrap_or_else(|poisoned| {
        log::error!("write lock was poisoned; recovering the last committed state");
        poisoned.into_inner()
    })
}

#[cfg(test)]
mod tests {
    use super::{mutex_lock_or_recover, write_or_recover};
    use std::sync::{Arc, Mutex, RwLock};

    #[test]
    fn poisoned_component_capture_mutex_is_recovered() {
        let value = Arc::new(Mutex::new(7_u8));
        let poison = Arc::clone(&value);
        let worker = std::thread::spawn(move || {
            let Ok(mut guard) = poison.lock() else {
                return;
            };
            *guard = 9;
            panic!("poison capture lock for recovery test");
        });
        assert!(worker.join().is_err());
        let guard = mutex_lock_or_recover(value.as_ref());
        assert_eq!(*guard, 9);
    }

    #[test]
    fn poisoned_project_write_lock_is_recovered() {
        let value = Arc::new(RwLock::new(7_u8));
        let poison = Arc::clone(&value);
        let worker = std::thread::spawn(move || {
            let Ok(mut guard) = poison.write() else {
                return;
            };
            *guard = 9;
            panic!("poison Project write lock for recovery test");
        });
        assert!(worker.join().is_err());
        let mut guard = write_or_recover(value.as_ref());
        *guard += 1;
        assert_eq!(*guard, 10);
    }
}

//! Lock access that recovers from poisoning.
//!
//! Not data vocabulary: it lives here because this is the one crate every other crate
//! depends on, so the helper reaches all of them without a new dependency edge. std only.
//!
//! Nothing in Kelvo relies on poisoning for correctness. The state behind its locks is
//! plain values updated in single steps, so a panic elsewhere while a guard was held
//! leaves it valid to use, and the guard is taken back with [`PoisonError::into_inner`].

use std::sync::{Mutex, MutexGuard, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// [`Mutex::lock`] that takes the guard back from a poisoned lock.
pub trait LockExt<T: ?Sized> {
    fn lock_ok(&self) -> MutexGuard<'_, T>;
}

impl<T: ?Sized> LockExt<T> for Mutex<T> {
    fn lock_ok(&self) -> MutexGuard<'_, T> {
        self.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// [`RwLock::read`] and [`RwLock::write`] that take the guard back from a poisoned lock.
pub trait RwLockExt<T: ?Sized> {
    fn read_ok(&self) -> RwLockReadGuard<'_, T>;
    fn write_ok(&self) -> RwLockWriteGuard<'_, T>;
}

impl<T: ?Sized> RwLockExt<T> for RwLock<T> {
    fn read_ok(&self) -> RwLockReadGuard<'_, T> {
        self.read().unwrap_or_else(PoisonError::into_inner)
    }

    fn write_ok(&self) -> RwLockWriteGuard<'_, T> {
        self.write().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use super::*;

    #[test]
    fn a_poisoned_mutex_still_hands_out_its_value() {
        let m = Mutex::new(1);
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let mut g = m.lock().unwrap();
            *g = 2;
            panic!("poison it");
        }));
        assert!(m.is_poisoned());
        *m.lock_ok() += 1;
        assert_eq!(*m.lock_ok(), 3);
    }

    #[test]
    fn a_poisoned_rwlock_still_hands_out_its_value() {
        let l = RwLock::new(1);
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _g = l.write().unwrap();
            panic!("poison it");
        }));
        assert!(l.is_poisoned());
        *l.write_ok() = 5;
        assert_eq!(*l.read_ok(), 5);
    }
}

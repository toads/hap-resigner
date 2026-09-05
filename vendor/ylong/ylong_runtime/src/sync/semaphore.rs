// Copyright (c) 2023 Huawei Device Co., Ltd.
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Asynchronous counting semaphore.

use crate::sync::semaphore_inner::{SemaphoreError, SemaphoreInner};

/// Asynchronous counting semaphore. It allows more than one caller to access
/// the shared resource. Semaphore contains a set of permits. Call `acquire`
/// method and get a permit to access the shared resource. When permits are used
/// up, new requests to acquire permit will wait until `release` method
/// is called. When no request is waiting, calling `release` method will add a
/// permit to semaphore.
///
/// The difference between [`AutoRelSemaphore`] and [`Semaphore`] is that permit
/// acquired from [`Semaphore`] will be consumed. When permit from
/// [`AutoRelSemaphore`] is dropped, it will be assigned to another acquiring
/// request or returned to the semaphore.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
///
/// use ylong_runtime::sync::semaphore::Semaphore;
///
/// async fn io_func() {
///     let sem = Arc::new(Semaphore::new(2).unwrap());
///     let sem2 = sem.clone();
///     let _permit1 = sem.try_acquire();
///     ylong_runtime::spawn(async move {
///         let _permit2 = sem2.acquire().await.unwrap();
///     });
/// }
/// ```
pub struct Semaphore {
    inner: SemaphoreInner,
}

/// Asynchronous counting semaphore. It allows more than one caller to access
/// the shared resource. semaphore contains a set of permits. Call `acquire`
/// method and get a permit to access the shared resource. The total number of
/// permits is fixed. When no permits are available, new request to
/// acquire permit will wait until another permit is dropped. When no request is
/// waiting and one permit is **dropped**, the permit will be return to
/// semaphore so that the number of permits in semaphore will increase.
///
/// The difference between [`AutoRelSemaphore`] and [`Semaphore`] is that permit
/// acquired from [`Semaphore`] will be consumed. When permit from
/// [`AutoRelSemaphore`] is dropped, it will be assigned to another acquiring
/// request or returned to the semaphore, in other words, permit will
/// be automatically released when it is dropped.
///
/// # Examples
///
/// ```
/// use std::sync::Arc;
///
/// use ylong_runtime::sync::semaphore::AutoRelSemaphore;
///
/// async fn io_func() {
///     let sem = Arc::new(AutoRelSemaphore::new(2).unwrap());
///     let sem2 = sem.clone();
///     let _permit1 = sem.try_acquire();
///     ylong_runtime::spawn(async move {
///         let _permit2 = sem2.acquire().await.unwrap();
///     });
/// }
/// ```
pub struct AutoRelSemaphore {
    inner: SemaphoreInner,
}

/// Permit acquired from `Semaphore`.
/// Consumed when dropped.
pub struct SemaphorePermit;

/// Permit acquired from `AutoRelSemaphore`.
/// Recycled when dropped.
pub struct AutoRelSemaphorePermit<'a> {
    sem: &'a AutoRelSemaphore,
}

impl Semaphore {
    /// Creates a `Semaphore` with an initial permit value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::sync::Semaphore;
    ///
    /// let sem = Semaphore::new(4).unwrap();
    /// ```
    pub fn new(permits: usize) -> Result<Semaphore, SemaphoreError> {
        match SemaphoreInner::new(permits) {
            Ok(inner) => Ok(Semaphore { inner }),
            Err(e) => Err(e),
        }
    }

    /// Gets the number of remaining permits.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::sync::Semaphore;
    /// let sem = Semaphore::new(4).unwrap();
    /// assert_eq!(sem.current_permits(), 4);
    /// ```
    pub fn current_permits(&self) -> usize {
        self.inner.current_permits()
    }

    /// Adds a permit to the semaphore.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::sync::Semaphore;
    ///
    /// let sem = Semaphore::new(4).unwrap();
    /// assert_eq!(sem.current_permits(), 4);
    /// sem.release();
    /// assert_eq!(sem.current_permits(), 5);
    /// ```
    pub fn release(&self) {
        self.inner.release();
    }

    /// Attempts to acquire a permit from semaphore.
    ///
    /// # Return value
    /// The function returns:
    ///  * `Ok(SemaphorePermit)` if acquiring a permit successfully.
    ///  * `Err(PermitError::Empty)` if no permit remaining in semaphore.
    ///  * `Err(PermitError::Closed)` if semaphore is closed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::sync::Semaphore;
    ///
    /// let sem = Semaphore::new(4).unwrap();
    /// assert_eq!(sem.current_permits(), 4);
    /// let permit = sem.try_acquire().unwrap();
    /// assert_eq!(sem.current_permits(), 3);
    /// drop(permit);
    /// assert_eq!(sem.current_permits(), 3);
    /// ```
    pub fn try_acquire(&self) -> Result<SemaphorePermit, SemaphoreError> {
        match self.inner.try_acquire() {
            Ok(_) => Ok(SemaphorePermit),
            Err(e) => Err(e),
        }
    }

    /// Asynchronously acquires a permit from semaphore.
    ///
    /// # Return value
    /// The function returns:
    ///  * `Ok(SemaphorePermit)` if acquiring a permit successfully.
    ///  * `Err(PermitError::Closed)` if semaphore is closed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::sync::Semaphore;
    /// async fn io_func() {
    ///     let sem = Semaphore::new(2).unwrap();
    ///     ylong_runtime::spawn(async move {
    ///         let _permit2 = sem.acquire().await.unwrap();
    ///     });
    /// }
    /// ```
    pub async fn acquire(&self) -> Result<SemaphorePermit, SemaphoreError> {
        self.inner.acquire().await?;
        Ok(SemaphorePermit)
    }

    /// Checks whether semaphore is closed. If so, the semaphore could not be
    /// acquired anymore.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::sync::semaphore::Semaphore;
    ///
    /// let sem = Semaphore::new(4).unwrap();
    /// assert!(!sem.is_closed());
    /// sem.close();
    /// assert!(sem.is_closed());
    /// ```
    pub fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }

    /// Closes the semaphore so that it could not be acquired anymore,
    /// and it notifies all requests in the waiting list.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::sync::Semaphore;
    ///
    /// let sem = Semaphore::new(4).unwrap();
    /// assert!(!sem.is_closed());
    /// sem.close();
    /// assert!(sem.is_closed());
    /// ```
    pub fn close(&self) {
        self.inner.close();
    }
}

impl AutoRelSemaphore {
    /// Creates a semaphore with an initial capacity.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::sync::AutoRelSemaphore;
    ///
    /// let sem = AutoRelSemaphore::new(4).unwrap();
    /// ```
    pub fn new(number: usize) -> Result<AutoRelSemaphore, SemaphoreError> {
        match SemaphoreInner::new(number) {
            Ok(inner) => Ok(AutoRelSemaphore { inner }),
            Err(e) => Err(e),
        }
    }

    /// Gets the number of remaining permits.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::sync::AutoRelSemaphore;
    /// let sem = AutoRelSemaphore::new(4).unwrap();
    /// assert_eq!(sem.current_permits(), 4);
    /// ```
    pub fn current_permits(&self) -> usize {
        self.inner.current_permits()
    }

    /// Attempts to acquire an auto-release-permit from semaphore.
    ///
    /// # Return value
    /// The function returns:
    ///  * `Ok(OneTimeSemaphorePermit)` if acquiring a permit successfully.
    ///  * `Err(PermitError::Empty)` if no permit remaining in semaphore.
    ///  * `Err(PermitError::Closed)` if semaphore is closed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::sync::AutoRelSemaphore;
    ///
    /// let sem = AutoRelSemaphore::new(4).unwrap();
    /// assert_eq!(sem.current_permits(), 4);
    /// let permit = sem.try_acquire().unwrap();
    /// assert_eq!(sem.current_permits(), 3);
    /// drop(permit);
    /// assert_eq!(sem.current_permits(), 4);
    /// ```
    pub fn try_acquire(&self) -> Result<AutoRelSemaphorePermit<'_>, SemaphoreError> {
        match self.inner.try_acquire() {
            Ok(_) => Ok(AutoRelSemaphorePermit { sem: self }),
            Err(e) => Err(e),
        }
    }

    /// Asynchronously acquires an auto-release-permit from semaphore.
    ///
    /// # Return value
    /// The function returns:
    ///  * `Ok(OneTimeSemaphorePermit)` if acquiring a permit successfully.
    ///  * `Err(PermitError::Closed)` if semaphore is closed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::sync::AutoRelSemaphore;
    ///
    /// async fn io_func() {
    ///     let sem = AutoRelSemaphore::new(2).unwrap();
    ///     ylong_runtime::spawn(async move {
    ///         let _permit2 = sem.acquire().await.unwrap();
    ///     });
    /// }
    /// ```
    pub async fn acquire(&self) -> Result<AutoRelSemaphorePermit<'_>, SemaphoreError> {
        self.inner.acquire().await?;
        Ok(AutoRelSemaphorePermit { sem: self })
    }

    /// Checks whether the state of semaphore is closed, if so, the semaphore
    /// could not acquire permits anymore.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::sync::AutoRelSemaphore;
    ///
    /// let sem = AutoRelSemaphore::new(4).unwrap();
    /// assert!(!sem.is_closed());
    /// sem.close();
    /// assert!(sem.is_closed());
    /// ```
    pub fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }

    /// Turns the state of semaphore to be closed so that semaphore could not
    /// acquire permits anymore, and notify all request in the waiting list.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::sync::AutoRelSemaphore;
    ///
    /// let sem = AutoRelSemaphore::new(4).unwrap();
    /// assert!(!sem.is_closed());
    /// sem.close();
    /// assert!(sem.is_closed());
    /// ```
    pub fn close(&self) {
        self.inner.close();
    }
}

impl Drop for AutoRelSemaphorePermit<'_> {
    fn drop(&mut self) {
        self.sem.inner.release();
    }
}
#[cfg(test)]
mod test {
    use std::sync::Arc;

    use crate::sync::{AutoRelSemaphore, Semaphore};
    use crate::task::JoinHandle;

    /// UT test cases for `Semaphore::close()`.
    ///
    /// # Brief
    /// 1. Create a counting semaphore with an initial capacity.
    /// 2. Check the semaphore is not closed.
    /// 3. Close the semaphore.
    /// 4. Check the semaphore is closed.
    #[test]
    fn ut_sem_close_test() {
        let sem = Semaphore::new(4).unwrap();
        assert!(!sem.is_closed());
        sem.close();
        assert!(sem.is_closed());
    }

    /// UT test cases for `AutoRelSemaphore::acquire()`.
    ///
    /// # Brief
    /// 1. Create a counting auto-release-semaphore with an initial capacity.
    /// 2. Acquire an auto-release-permit.
    /// 3. Asynchronously acquires a permit.
    /// 4. Check the number of permits in every stage.
    #[test]
    fn ut_auto_release_sem_acquire_test() {
        let sem = Arc::new(AutoRelSemaphore::new(1).unwrap());
        let sem2 = sem.clone();
        let handle = crate::spawn(async move {
            let _permit2 = sem2.acquire().await.unwrap();
            assert_eq!(sem2.current_permits(), 0);
        });
        crate::block_on(handle).expect("block_on failed");
        assert_eq!(sem.current_permits(), 1);
    }

    /// UT test cases for `Semaphore::release()`.
    ///
    /// # Brief
    /// 1. Create a counting semaphore with an initial capacity.
    /// 2. Call `Semaphore::release()` to add a permit to the semaphore.
    /// 3. Check the number of permits before and after releasing.
    #[test]
    fn ut_release_test() {
        let sem = Semaphore::new(2).unwrap();
        assert_eq!(sem.current_permits(), 2);
        sem.release();
        assert_eq!(sem.current_permits(), 3);
    }

    /// UT test cases for `AutoRelSemaphore::close()`.
    ///
    /// # Brief
    /// 1. Create a counting auto-release-semaphore with an initial capacity.
    /// 2. Close the semaphore.
    /// 3. Fail to acquire an auto-release-permit.
    #[test]
    fn ut_auto_release_sem_close_test() {
        let sem = Arc::new(AutoRelSemaphore::new(2).unwrap());
        let sem2 = sem.clone();
        assert!(!sem.is_closed());
        sem.close();
        assert!(sem.is_closed());
        let permit = sem.try_acquire();
        assert!(permit.is_err());
        let handle = crate::spawn(async move {
            let permit2 = sem2.acquire().await;
            assert!(permit2.is_err());
        });
        crate::block_on(handle).expect("block_on failed");
    }

    /// Stress test cases for `AutoRelSemaphore::acquire()`.
    ///
    /// # Brief
    /// 1. Create a counting auto-release-semaphore with an initial capacity.
    /// 2. Repeating acquiring an auto-release-permit for a huge number of
    ///    times.
    /// 3. Check the correctness of function of semaphore.
    #[test]
    fn ut_auto_release_sem_stress_test() {
        let sem = Arc::new(AutoRelSemaphore::new(5).unwrap());
        let mut tasks: Vec<JoinHandle<()>> = Vec::new();

        for _ in 0..1000 {
            let sem2 = sem.clone();
            tasks.push(crate::spawn(async move {
                let _permit = sem2.acquire().await;
            }));
        }
        for t in tasks {
            let _ = crate::block_on(t);
        }
        let permit1 = sem.try_acquire();
        assert!(permit1.is_ok());
        let permit2 = sem.try_acquire();
        assert!(permit2.is_ok());
        let permit3 = sem.try_acquire();
        assert!(permit3.is_ok());
        let permit4 = sem.try_acquire();
        assert!(permit4.is_ok());
        let permit5 = sem.try_acquire();
        assert!(permit5.is_ok());
        assert!(sem.try_acquire().is_err());
    }

    /// Stress test cases for `AutoRelSemaphore::acquire()` and
    /// `AutoRelSemaphore::drop()`.
    ///
    /// # Brief
    /// 1. Create a counting auto-release-semaphore with an initial capacity.
    /// 2. Repeating acquiring a pair of auto-release-permit for a few times.
    /// 3. Check the correctness of the future of `Permit`.
    #[test]
    fn ut_async_stress_test() {
        let mut tasks: Vec<JoinHandle<()>> = Vec::new();

        for _ in 0..50 {
            let sem = Arc::new(AutoRelSemaphore::new(1).unwrap());
            let sem2 = sem.clone();
            tasks.push(crate::spawn(async move {
                let _permit = sem.acquire().await;
            }));
            tasks.push(crate::spawn(async move {
                let _permit = sem2.acquire().await;
            }));
        }
        for t in tasks {
            let _ = crate::block_on(t);
        }
    }

    /// UT test cases for `Semaphore::try_acquire()`.
    ///
    /// # Brief
    /// 1. Create a counting semaphore with an initial capacity.
    /// 2. Acquire permits successfully.
    /// 3. Fail to acquire a permit when all permits are consumed.
    #[test]
    fn ut_try_acquire_test() {
        let sem = Semaphore::new(2).unwrap();
        let permit = sem.try_acquire();
        assert!(permit.is_ok());
        drop(permit);
        assert_eq!(sem.current_permits(), 1);
        let permit2 = sem.try_acquire();
        assert!(permit2.is_ok());
        drop(permit2);
        assert_eq!(sem.current_permits(), 0);
        let permit3 = sem.try_acquire();
        assert!(permit3.is_err());
    }

    /// UT test cases for `Semaphore::acquire()`.
    ///
    /// # Brief
    /// 1. Create a counting semaphore with an initial capacity.
    /// 2. Acquire a permit.
    /// 3. Asynchronously acquires a permit.
    /// 4. Check the number of permits in every stage.
    #[test]
    fn ut_acquire_test() {
        let sem = Arc::new(Semaphore::new(0).unwrap());
        let sem2 = sem.clone();
        let handle = crate::spawn(async move {
            let _permit2 = sem2.acquire().await.unwrap();
            assert_eq!(sem2.current_permits(), 0);
        });
        sem.release();
        crate::block_on(handle).expect("block_on failed");
        assert_eq!(sem.current_permits(), 0);
    }
}

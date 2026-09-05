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

use std::convert::TryInto;
use std::future::Future;
use std::marker::PhantomPinned;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

const TEN_YEARS: Duration = Duration::from_secs(86400 * 365 * 10);

/// Waits until 'instant' has reached.
///
/// # Panic
/// Calling this method outside of a Ylong Runtime could cause panic, for
/// example, outside of an async closure that is passed to ylong_runtime::spawn
/// or ylong_runtime::block_on. The async wrapping is necessary since it makes
/// the function become lazy in order to get successfully executed on the
/// runtime.
pub fn sleep_until(instant: Instant) -> Sleep {
    Sleep::new_timeout(instant)
}

/// Waits until 'duration' has elapsed.
///
/// # Panic
/// Calling this method outside of a Ylong Runtime could cause panic, for
/// example, outside of an async closure that is passed to ylong_runtime::spawn
/// or ylong_runtime::block_on. The async wrapping is necessary since it makes
/// the function become lazy in order to get successfully executed on the
/// runtime.
pub fn sleep(duration: Duration) -> Sleep {
    // If the time reaches the maximum value,
    // then set the default timing time to 10 years.
    match Instant::now().checked_add(duration) {
        Some(deadline) => Sleep::new_timeout(deadline),
        None => Sleep::new_timeout(Instant::now() + TEN_YEARS),
    }
}

/// A structure that implements Future. returned by func [`sleep`].
///
/// [`sleep`]: sleep
/// # Examples
///
/// ```
/// use std::time::Duration;
///
/// use ylong_runtime::time::sleep;
///
/// async fn sleep_test() {
///     let sleep = sleep(Duration::from_secs(2)).await;
///     println!("2 secs have elapsed");
/// }
/// ```
pub struct Sleep {
    // During the polling of this structure, no repeated insertion.
    need_insert: bool,

    // The time at which the structure should end.
    deadline: Instant,

    inner: SleepInner,

    _phantom: PhantomPinned,
}

cfg_ffrt!(
    use crate::ffrt::ffrt_timer::FfrtTimerEntry;
    use std::task::Waker;

    struct SleepInner {
        // ffrt timer handle
        timer: Option<FfrtTimerEntry>,
        // the waker to wakeup the timer task
        waker: Option<*mut Waker>,
    }

    // FFRT needs this unsafe impl since `Sleep` has a mut pointer in it.
    // In non-ffrt environment, `Sleep` auto-derives Sync & Send.
    unsafe impl Send for Sleep {}
    unsafe impl Sync for Sleep {}

    impl Sleep {
        // Creates a Sleep structure based on the given deadline.
        fn new_timeout(deadline: Instant) -> Self {
            Self {
                need_insert: true,
                deadline,
                inner: SleepInner {
                    timer: None,
                    waker: None,
                }
            }
        }

        // Resets the deadline of the Sleep
        pub(crate) fn reset(&mut self, new_deadline: Instant) {
            self.need_insert = true;
            self.deadline = new_deadline;

            if let Some(waker) = self.inner.waker.take() {
                unsafe {
                    drop(Box::from_raw(waker));
                }
            }
        }

        // Cancels the Sleep
        fn cancel(&mut self) {
            if let Some(timer) = self.inner.timer.take() {
                timer.timer_deregister();
            }
            if let Some(waker) = self.inner.waker.take() {
                unsafe {
                    drop(Box::from_raw(waker));
                }
            }
        }
    }

    impl Future for Sleep {
        type Output = ();

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = self.get_mut();

            if this.need_insert {
                if let Some(duration) = this.deadline.checked_duration_since(Instant::now()) {
                    let ms = duration.as_millis()
                    .try_into()
                    .unwrap_or(u64::MAX);

                    let waker = Box::new(cx.waker().clone());
                    let waker_ptr = Box::into_raw(waker);

                    if let Some(waker) = this.inner.waker.take() {
                        unsafe { drop(Box::from_raw(waker)); }
                    }

                    this.inner.waker = Some(waker_ptr);
                    this.inner.timer = Some(FfrtTimerEntry::timer_register(waker_ptr, ms));
                    this.need_insert = false;
                } else {
                    return Poll::Ready(());
                }
            }

            // this unwrap is safe since we have already insert the timer into the entry
            let timer = this.inner.timer.as_ref().unwrap();
            if timer.result() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }
    }
);

impl Sleep {
    // Returns the deadline of the Sleep
    pub(crate) fn deadline(&self) -> Instant {
        self.deadline
    }
}

cfg_not_ffrt!(
    use crate::executor::driver::Handle;
    use crate::time::Clock;
    use std::sync::Arc;
    use std::cmp;
    use std::ptr::NonNull;

    struct SleepInner {
        // Corresponding Timer structure.
        timer: Clock,
        // Timer driver handle
        handle: Arc<Handle>,
    }

    impl Sleep {
        // Creates a Sleep structure based on the given deadline.
        fn new_timeout(deadline: Instant) -> Self {
            let handle = Handle::get_handle().unwrap_or_else(|e| panic!("sleep new out of worker ctx, error: {e}"));

            let start_time = handle.start_time();
            let deadline = cmp::max(deadline, start_time);

            let timer = Clock::new();
            Self {
                need_insert: true,
                deadline,
                inner: SleepInner {
                    timer,
                    handle,
                },
                _phantom: PhantomPinned,
            }
        }

        // Resets the deadline of the Sleep
        pub(crate) fn reset(self: Pin<&mut Self>, new_deadline: Instant) {
            let this = unsafe { self.get_unchecked_mut() };
            this.need_insert = true;
            this.deadline = new_deadline;
            this.inner.timer.set_result(false);
        }

        // Cancels the Sleep
        fn cancel(&mut self) {
            let driver = &self.inner.handle;
            driver.timer_cancel(NonNull::from(&self.inner.timer));
        }
    }

    impl Future for Sleep {
        type Output = ();

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let this = unsafe { self.get_unchecked_mut() };
            let driver = &this.inner.handle;

            if this.need_insert {
                // the deadline is guaranteed to be later than the start time
                let ms = this
                    .deadline
                    .checked_duration_since(driver.start_time())
                    .unwrap()
                    .as_millis()
                    .try_into()
                    .unwrap_or(u64::MAX);
                this.inner.timer.set_expiration(ms);
                this.inner.timer.set_waker(cx.waker().clone());

                match driver.timer_register(NonNull::from(&this.inner.timer)) {
                    Ok(_) => this.need_insert = false,
                    Err(_) => {
                        // Even if the insertion fails, there is no need to insert again here,
                        // it is a timeout clock and needs to be triggered immediately at the next poll.
                        this.need_insert = false;
                        this.inner.timer.set_result(true);
                    }
                }
            }

            if this.inner.timer.result() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        }
    }
);

impl Drop for Sleep {
    fn drop(&mut self) {
        // For some uses, for example, Timeout,
        // `Sleep` enters the `Pending` state first and inserts the `TimerHandle` into
        // the `DRIVER`, the future of timeout returns `Ready` in advance of the
        // next polling, as a result, the `TimerHandle` pointer in the `DRIVER`
        // is invalid. need to cancel the `TimerHandle` operation during `Sleep`
        // drop.
        self.cancel()
    }
}

#[cfg(test)]
mod test {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use crate::time::{sleep, sleep_until};
    use crate::{block_on, spawn};

    /// UT test cases for new_sleep
    ///
    /// # Brief
    /// 1. Uses sleep to create a Sleep Struct.
    /// 2. Uses block_on to test different sleep duration.
    #[test]
    fn ut_new_timer_sleep() {
        let val = Arc::new(AtomicUsize::new(0));
        let val_cpy = val.clone();
        block_on(async move {
            sleep(Duration::new(0, 20_000_000)).await;
            sleep(Duration::new(0, 20_000_000)).await;
            sleep(Duration::new(0, 20_000_000)).await;
            val_cpy.fetch_add(1, Ordering::Relaxed);
        });

        assert_eq!(val.load(Ordering::Relaxed), 1);
        let val_cpy2 = val.clone();
        let val_cpy3 = val.clone();
        let val_cpy4 = val.clone();
        let handle_one = spawn(async move {
            sleep(Duration::new(0, 20_000_000)).await;
            val_cpy2.fetch_add(1, Ordering::Relaxed);
        });
        let handle_two = spawn(async move {
            sleep(Duration::new(0, 20_000_000)).await;
            val_cpy3.fetch_add(1, Ordering::Relaxed);
        });
        let handle_three = spawn(async move {
            sleep(Duration::new(0, 20_000_000)).await;
            val_cpy4.fetch_add(1, Ordering::Relaxed);
        });
        block_on(handle_one).unwrap();
        block_on(handle_two).unwrap();
        block_on(handle_three).unwrap();
        assert_eq!(val.load(Ordering::Relaxed), 4);
    }

    /// UT test cases for sleep zero second or sleep until a past instant
    ///
    /// # Brief
    /// 1. Call sleep with a duration of zero, check if the val is successfully
    ///    added.
    /// 2. Call sleep with a past instant, check if the val is successfully
    ///    added.
    #[test]
    fn ut_timer_sleep_zero() {
        let mut val = 0;
        let past = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
        let mut val = block_on(async move {
            sleep(Duration::new(0, 0)).await;
            val += 1;
            val
        });
        assert_eq!(val, 1);

        let val = block_on(async move {
            sleep_until(past).await;
            val += 1;
            val
        });
        assert_eq!(val, 2);
    }
}

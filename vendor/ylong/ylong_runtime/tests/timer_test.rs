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

#![cfg(all(feature = "time", feature = "sync"))]

use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use ylong_runtime::time::{sleep, sleep_until, Sleep};

async fn download() {
    const TOTAL_SIZE: usize = 10 * 1024;
    const RECV_SIZE: usize = 1024;

    let mut left = TOTAL_SIZE;
    loop {
        let recv = RECV_SIZE;
        left -= recv;
        if left == 0 {
            break;
        }
        sleep(Duration::from_millis(50)).await;
    }
}

async fn simulate() {
    let mut handles = Vec::new();

    for _ in 0..50 {
        handles.push(ylong_runtime::spawn(async move {
            download().await;
        }));
    }

    for handle in handles {
        let _ = handle.await;
    }
}

/// SDV test cases for multi time create.
///
/// # Brief
/// 1. Creates multi threads and multi timers.
/// 2. Checks if the test results are correct.
#[test]
fn sdv_multi_timer() {
    ylong_runtime::block_on(simulate());
}

/// SDV for dropping timer outside of worker context
///
/// # Brief
/// 1. Creates a `Sleep` on the worker context
/// 2. Returns the sleep to the main thread which is not in the worker context
/// 3. Drops the timer in the main thread and it should not cause Panic
#[test]
#[allow(clippy::async_yields_async)]
fn sdv_sleep_drop_out_context() {
    let handle = ylong_runtime::spawn(async move { sleep_until(Instant::now()) });
    let timer = ylong_runtime::block_on(handle).unwrap();
    drop(timer);
}

/// SDV case for calling `block_on` directly on a `timeout` operation
///
/// # Brief
/// 1. Blocks on the async function that times out
/// 2. Checks if the returned value is Ok
#[test]
#[cfg(not(feature = "ffrt"))]
fn sdv_block_on_timeout() {
    use ylong_runtime::time::timeout;

    let ret =
        ylong_runtime::block_on(
            async move { timeout(Duration::from_secs(2), async move { 1 }).await },
        );
    assert_eq!(ret, Ok(1))
}

struct TimeFuture {
    sleep: Option<Pin<Box<Sleep>>>,
    count: u32,
}

impl Future for TimeFuture {
    type Output = u32;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();
        if this.count < 5 {
            this.sleep = Some(Box::pin(sleep(Duration::from_millis(50))));
        }
        if let Some(mut sleep) = this.sleep.take() {
            if Pin::new(&mut sleep).poll(cx).is_pending() {
                this.count += 1;
                this.sleep = Some(sleep);
                return Poll::Pending;
            }
        }
        Poll::Ready(0)
    }
}

/// SDV case for polling a sleep
///
/// # Brief
/// 1. Create a future that would create a new sleep during every call to its
///    `poll`
/// 2. Spawn and block_on the future, check if the returned value is correct
#[test]
fn sdv_sleep_poll() {
    let handle = ylong_runtime::spawn(async move {
        for _ in 0..2 {
            let future = TimeFuture {
                sleep: None,
                count: 0,
            };
            let ret = future.await;
            assert_eq!(ret, 0);
        }
        1
    });
    let ret = ylong_runtime::block_on(handle).unwrap();
    assert_eq!(ret, 1);
}

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

//! Benchmarks for signal.
//!
//! Designs of ylong_runtime benchmarks:
//! - Multiple threads listen to the same signal and wake up once when all
//!   threads are waiting for the signal.
//! - A single thread loops to wait for a signal, and the loop notifies until
//!   all waiting is completed.

#![feature(test)]

pub mod task_helpers;

#[macro_export]
macro_rules! tokio_signal_multi_thread_task {
    ($runtime: expr, $bench: ident, $kind: expr, $sig: ident, $num: literal) => {
        #[bench]
        fn $bench(b: &mut Bencher) {
            let runtime = $runtime;

            b.iter(black_box(|| {
                let num = Arc::new(AtomicUsize::new(0));
                let mut handlers = Vec::with_capacity($num);
                for _ in 0..$num {
                    let num_clone = num.clone();
                    handlers.push(runtime.spawn(async move {
                        let mut stream = tokio_signal($kind).unwrap();
                        num_clone.fetch_add(1, Release);
                        stream.recv().await;
                    }));
                }
                while num.load(Acquire) < 10 {}
                unsafe { libc::raise($sig) };
                for handler in handlers {
                    let _ = runtime.block_on(handler).unwrap();
                }
            }));
        }
    };
}

#[macro_export]
macro_rules! tokio_signal_single_thread_task {
    ($runtime: expr, $bench: ident, $kind: expr, $sig: ident, $num: literal) => {
        #[bench]
        fn $bench(b: &mut Bencher) {
            let runtime = $runtime;

            b.iter(black_box(|| {
                let handler = runtime.spawn(async move {
                    let mut stream = tokio_signal($kind).unwrap();
                    for _ in 0..$num {
                        unsafe { libc::raise($sig) };
                        stream.recv().await;
                    }
                });
                let _ = runtime.block_on(handler).unwrap();
            }));
        }
    };
}

#[macro_export]
macro_rules! ylong_signal_multi_thread_task {
    ($runtime: expr, $bench: ident, $kind: expr, $sig: ident, $num: literal) => {
        #[bench]
        fn $bench(b: &mut Bencher) {
            let runtime = $runtime;

            b.iter(black_box(|| {
                let num = Arc::new(AtomicUsize::new(0));
                let mut handlers = Vec::with_capacity($num);
                for _ in 0..$num {
                    let num_clone = num.clone();
                    handlers.push(runtime.spawn(async move {
                        let mut stream = ylong_signal($kind).unwrap();
                        num_clone.fetch_add(1, Release);
                        stream.recv().await;
                    }));
                }
                while num.load(Acquire) < 10 {}
                unsafe { libc::raise($sig) };
                for handler in handlers {
                    let _ = runtime.block_on(handler).unwrap();
                }
            }));
        }
    };
}

#[macro_export]
macro_rules! ylong_signal_single_thread_task {
    ($runtime: expr, $bench: ident, $kind: expr, $sig: ident, $num: literal) => {
        #[bench]
        fn $bench(b: &mut Bencher) {
            let runtime = $runtime;

            b.iter(black_box(|| {
                let handler = runtime.spawn(async move {
                    let mut stream = ylong_signal($kind).unwrap();
                    for _ in 0..$num {
                        unsafe { libc::raise($sig) };
                        stream.recv().await;
                    }
                });
                let _ = runtime.block_on(handler).unwrap();
            }));
        }
    };
}

#[cfg(test)]
mod signal_bench {
    extern crate test;

    use std::hint::black_box;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering::{Acquire, Release};
    use std::sync::Arc;

    use libc::{SIGALRM, SIGHUP, SIGIO, SIGPIPE};
    use test::Bencher;
    use tokio::signal::unix::{signal as tokio_signal, SignalKind as TokioSignalKind};
    #[cfg(feature = "signal")]
    use ylong_runtime::signal::{signal as ylong_signal, SignalKind as YlongSignalKind};

    pub use crate::task_helpers::{tokio_runtime, ylong_runtime};

    ylong_signal_single_thread_task!(
        ylong_runtime(),
        ylong_signal_single_thread_10,
        YlongSignalKind::hangup(),
        SIGHUP,
        10
    );
    ylong_signal_multi_thread_task!(
        ylong_runtime(),
        ylong_signal_multi_thread_10,
        YlongSignalKind::alarm(),
        SIGALRM,
        10
    );
    tokio_signal_single_thread_task!(
        tokio_runtime(),
        tokio_signal_single_thread_10,
        TokioSignalKind::pipe(),
        SIGPIPE,
        10
    );
    tokio_signal_multi_thread_task!(
        tokio_runtime(),
        tokio_signal_multi_thread_10,
        TokioSignalKind::io(),
        SIGIO,
        10
    );
}

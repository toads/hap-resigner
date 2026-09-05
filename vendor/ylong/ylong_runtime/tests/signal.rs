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

#![cfg(feature = "signal")]

#[cfg(unix)]
mod linux_test {
    use std::os::raw::c_int;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering::{Acquire, Release};
    use std::sync::Arc;

    use ylong_runtime::futures::poll_fn;
    use ylong_runtime::signal::{signal, SignalKind};

    /// SDV cases of `SignalKind` conversion.
    ///
    /// # Brief
    /// 1. Check the trait `From<c_int>` for `SignalKind`.
    /// 2. Check the trait `From<SignalKind>` for `c_int`.
    /// 3. Check the method `from_raw` of `SignalKind`.
    /// 4. Check the method `as_raw` of `SignalKind`.
    #[test]
    fn sdv_signal_from_and_into_c_int() {
        assert_eq!(SignalKind::from(1), SignalKind::hangup());
        assert_eq!(c_int::from(SignalKind::hangup()), 1);
        assert_eq!(SignalKind::from_raw(2), SignalKind::interrupt());
        assert_eq!(SignalKind::interrupt().as_raw(), 2);
    }

    /// SDV cases for signal `recv()`.
    ///
    /// # Brief
    /// 1. Generate a counter to ensure that notifications are received every
    ///    time listening.
    /// 2. Spawns a task to loop and listen to a signal.
    /// 3. Send notification signals in a loop until all waiting is completed.
    #[test]
    fn sdv_signal_recv_test() {
        let handle = ylong_runtime::spawn(async move {
            let mut stream = signal(SignalKind::alarm()).unwrap();
            for _ in 0..10 {
                unsafe { libc::raise(libc::SIGALRM) };
                stream.recv().await;
            }
        });
        let _ = ylong_runtime::block_on(handle);
    }

    /// SDV cases for signal `recv()` in multi thread.
    ///
    /// # Brief
    /// 1. Generate a counter to confirm that all signals are waiting.
    /// 2. Spawns some tasks to listen to a signal.
    /// 3. Send a notification signal when all signals are waiting.
    #[test]
    fn sdv_signal_recv_multi_thread_test() {
        let num = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..10 {
            let num_clone = num.clone();
            handles.push(ylong_runtime::spawn(async move {
                let mut stream = signal(SignalKind::child()).unwrap();
                num_clone.fetch_add(1, Release);
                stream.recv().await;
            }));
        }
        while num.load(Acquire) < 10 {}
        unsafe { libc::raise(libc::SIGCHLD) };
        for handle in handles {
            let _ = ylong_runtime::block_on(handle);
        }
    }

    /// SDV cases for signal `poll_recv()`.
    ///
    /// # Brief
    /// 1. Generate a counter to ensure that notifications are received every
    ///    time listening.
    /// 2. Spawns a task to loop and listen to a signal.
    /// 3. Send notification signals in a loop until all waiting is completed.
    #[test]
    fn sdv_signal_poll_recv_test() {
        let handle = ylong_runtime::spawn(async move {
            let mut stream = signal(SignalKind::hangup()).unwrap();
            for _ in 0..10 {
                unsafe { libc::raise(libc::SIGHUP) };
                poll_fn(|cx| stream.poll_recv(cx)).await;
            }
        });
        let _ = ylong_runtime::block_on(handle);
    }

    /// SDV cases for signal `poll_recv()` in multi thread.
    ///
    /// # Brief
    /// 1. Generate a counter to confirm that all signals are waiting.
    /// 2. Spawns some tasks to listen to a signal.
    /// 3. Send a notification signal when all signals are waiting.
    #[test]
    fn sdv_signal_poll_recv_multi_thread_test() {
        let num = Arc::new(AtomicUsize::new(0));
        let mut handles = Vec::new();
        for _ in 0..10 {
            let num_clone = num.clone();
            handles.push(ylong_runtime::spawn(async move {
                let mut stream = signal(SignalKind::io()).unwrap();
                num_clone.fetch_add(1, Release);
                stream.recv().await;
            }));
        }
        while num.load(Acquire) < 10 {}
        unsafe { libc::raise(libc::SIGIO) };
        for handle in handles {
            let _ = ylong_runtime::block_on(handle);
        }
    }

    /// SDV cases for SIGALRM signal.
    ///
    /// # Brief
    /// 1. Generate the SIGALRM signal.
    /// 2. Check the function of `signal` for the SIGALRM signal.
    #[test]
    fn sdv_signal_alarm() {
        let handle = ylong_runtime::spawn(async move {
            let mut signal = signal(SignalKind::alarm()).unwrap();
            unsafe { libc::raise(libc::SIGALRM) };
            signal.recv().await;
        });
        let _ = ylong_runtime::block_on(handle);
    }

    /// SDV cases for SIGCHLD signal.
    ///
    /// # Brief
    /// 1. Generate the SIGCHLD signal.
    /// 2. Check the function of `signal` for the SIGCHLD signal.
    #[test]
    fn sdv_signal_child() {
        let handle = ylong_runtime::spawn(async move {
            let mut signal = signal(SignalKind::child()).unwrap();
            unsafe { libc::raise(libc::SIGCHLD) };
            signal.recv().await;
        });
        let _ = ylong_runtime::block_on(handle);
    }

    /// SDV cases for SIGHUP signal.
    ///
    /// # Brief
    /// 1. Generate the SIGHUP signal.
    /// 2. Check the function of `signal` for the SIGHUP signal.
    #[test]
    fn sdv_signal_hangup() {
        let handle = ylong_runtime::spawn(async move {
            let mut signal = signal(SignalKind::hangup()).unwrap();
            unsafe { libc::raise(libc::SIGHUP) };
            signal.recv().await;
        });
        let _ = ylong_runtime::block_on(handle);
    }

    /// SDV cases for SIGINT signal.
    ///
    /// # Brief
    /// 1. Generate the SIGINT signal.
    /// 2. Check the function of `signal` for the SIGINT signal.
    #[test]
    fn sdv_signal_interrupt() {
        let handle = ylong_runtime::spawn(async move {
            let mut signal = signal(SignalKind::interrupt()).unwrap();
            unsafe { libc::raise(libc::SIGINT) };
            signal.recv().await;
        });
        let _ = ylong_runtime::block_on(handle);
    }

    /// SDV cases for SIGIO signal.
    ///
    /// # Brief
    /// 1. Generate the SIGIO signal.
    /// 2. Check the function of `signal` for the SIGIO signal.
    #[test]
    fn sdv_signal_io() {
        let handle = ylong_runtime::spawn(async move {
            let mut signal = signal(SignalKind::io()).unwrap();
            unsafe { libc::raise(libc::SIGIO) };
            signal.recv().await;
        });
        let _ = ylong_runtime::block_on(handle);
    }

    /// SDV cases for SIGPIPE signal.
    ///
    /// # Brief
    /// 1. Generate the SIGPIPE signal.
    /// 2. Check the function of `signal` for the SIGPIPE signal.
    #[test]
    fn sdv_signal_pipe() {
        let handle = ylong_runtime::spawn(async move {
            let mut signal = signal(SignalKind::pipe()).unwrap();
            unsafe { libc::raise(libc::SIGPIPE) };
            signal.recv().await;
        });
        let _ = ylong_runtime::block_on(handle);
    }

    /// SDV cases for SIGTERM signal.
    ///
    /// # Brief
    /// 1. Generate the SIGTERM signal.
    /// 2. Check the function of `signal` for the SIGTERM signal.
    #[test]
    fn sdv_signal_terminate() {
        let handle = ylong_runtime::spawn(async move {
            let mut signal = signal(SignalKind::terminate()).unwrap();
            unsafe { libc::raise(libc::SIGTERM) };
            signal.recv().await;
        });
        let _ = ylong_runtime::block_on(handle);
    }

    /// SDV cases for SIGUSR1 signal.
    ///
    /// # Brief
    /// 1. Generate the SIGUSR1 signal.
    /// 2. Check the function of `signal` for the SIGUSR1 signal.
    #[test]
    fn sdv_signal_user_defined1() {
        let handle = ylong_runtime::spawn(async move {
            let mut signal = signal(SignalKind::user_defined1()).unwrap();
            unsafe { libc::raise(libc::SIGUSR1) };
            signal.recv().await;
        });
        let _ = ylong_runtime::block_on(handle);
    }

    /// SDV cases for SIGUSR2 signal.
    ///
    /// # Brief
    /// 1. Generate the SIGUSR2 signal.
    /// 2. Check the function of `signal` for the SIGUSR2 signal.
    #[test]
    fn sdv_signal_user_defined2() {
        let handle = ylong_runtime::spawn(async move {
            let mut signal = signal(SignalKind::user_defined2()).unwrap();
            unsafe { libc::raise(libc::SIGUSR2) };
            signal.recv().await;
        });
        let _ = ylong_runtime::block_on(handle);
    }

    /// SDV cases for SIGWINCH signal.
    ///
    /// # Brief
    /// 1. Generate the SIGWINCH signal.
    /// 2. Check the function of `signal` for the SIGWINCH signal.
    #[test]
    fn sdv_signal_window_change() {
        let handle = ylong_runtime::spawn(async move {
            let mut signal = signal(SignalKind::window_change()).unwrap();
            unsafe { libc::raise(libc::SIGWINCH) };
            signal.recv().await;
        });
        let _ = ylong_runtime::block_on(handle);
    }
}

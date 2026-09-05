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

use std::io;
use std::mem::MaybeUninit;
use std::os::raw::c_int;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Once};

/// SDV test cases
///
/// Because the following tests cannot be executed in parallel for
/// there are only a few signals in windows that we have to use the same signal
/// in different test case, so there is a test all case which execute all tests
/// serially.
#[test]
fn sdv_test_all() {
    sdv_signal_register_succeed();
    sdv_signal_register_failed();
    sdv_signal_register_with_old();
    #[cfg(not(windows))]
    sdv_signal_register_multi();
}

/// SDV cases for signal register
///
/// # Brief
/// 1. Registers two different signals with actions that increment two different
///    atomic usize.
/// 2. Manually raises the two signals, checks if the registered action behave
///    correctly.
/// 3. Deregisters the action of the two signals
/// 4. Registers the same action for one of the signals again
/// 5. Manually raises the signal, checks if the registered action behave
///    correctly
/// 6. Deregisters both signal's handler hook, checks if the return is ok.
fn sdv_signal_register_succeed() {
    let value = Arc::new(AtomicUsize::new(0));
    let value_cpy = value.clone();

    let value2 = Arc::new(AtomicUsize::new(10));
    let value2_cpy = value2.clone();
    let value2_cpy2 = value2.clone();

    let res = unsafe {
        ylong_signal::register_signal_action(libc::SIGINT, move || {
            value_cpy.fetch_add(1, Ordering::Relaxed);
        })
    };
    assert!(res.is_ok());

    let res = unsafe {
        ylong_signal::register_signal_action(libc::SIGTERM, move || {
            value2_cpy.fetch_add(10, Ordering::Relaxed);
        })
    };
    assert!(res.is_ok());
    assert_eq!(value.load(Ordering::Relaxed), 0);

    unsafe { libc::raise(libc::SIGINT) };
    assert_eq!(value.load(Ordering::Relaxed), 1);
    assert_eq!(value2.load(Ordering::Relaxed), 10);

    unsafe { libc::raise(libc::SIGTERM) };
    assert_eq!(value.load(Ordering::Relaxed), 1);
    assert_eq!(value2.load(Ordering::Relaxed), 20);

    let res = ylong_signal::deregister_signal_action(libc::SIGTERM);
    assert!(res.is_ok());

    ylong_signal::deregister_signal_action(libc::SIGINT).unwrap();

    let res = unsafe {
        ylong_signal::register_signal_action(libc::SIGTERM, move || {
            value2_cpy2.fetch_add(20, Ordering::Relaxed);
        })
    };
    assert!(res.is_ok());

    unsafe { libc::raise(libc::SIGTERM) };
    assert_eq!(value2.load(Ordering::Relaxed), 40);

    let res = ylong_signal::deregister_signal_hook(libc::SIGTERM);
    assert!(res.is_ok());

    let res = ylong_signal::deregister_signal_hook(libc::SIGINT);
    assert!(res.is_ok());
}

/// SDV cases for signal register error handling
///
/// # Brief
/// 1. Registers an action for a forbidden signal
/// 2. Checks if the return value is InvalidInput error
/// 3. Registers an action for an allowed signal
/// 4. Checks if the return value is Ok
/// 5. Registers an action for the same signal again
/// 6. Checks if the return value is AlreadyExists error
/// 7. Deregisters the signal hook of the previous registered signal
/// 8. Checks if the return value is OK
/// 9. Deregisters the signal action of an unregistered signal
/// 10. Deregisters the signal handler of an unregistered signal
/// 11. Checks if the return value is Ok
fn sdv_signal_register_failed() {
    let res = unsafe { ylong_signal::register_signal_action(libc::SIGSEGV, move || {}) };
    assert_eq!(res.unwrap_err().kind(), io::ErrorKind::InvalidInput);

    let res = unsafe { ylong_signal::register_signal_action(libc::SIGTERM, move || {}) };
    assert!(res.is_ok());
    let res = unsafe { ylong_signal::register_signal_action(libc::SIGTERM, move || {}) };
    assert_eq!(res.unwrap_err().kind(), io::ErrorKind::AlreadyExists);

    let res = ylong_signal::deregister_signal_hook(libc::SIGTERM);
    assert!(res.is_ok());

    let res = ylong_signal::deregister_signal_action(libc::SIGSEGV);
    assert!(res.is_ok());

    let res = ylong_signal::deregister_signal_hook(libc::SIGSEGV);
    assert!(res.is_ok());
}

/// SDV cases for signal register when there is already an existing handler
///
/// # Brief
/// 1. Registers a signal handler using libc syscall
/// 2. Registers a signal handler using ylong_signal::register_signal_action
/// 3. Manually raises the signal
/// 4. Checks if the the new action get executed correctly
/// 5. Deregisters the signal action
/// 6. Manually raises the signal
/// 7. Checks if the old handler gets executed correctly
/// 8. Deregister the hook.
fn sdv_signal_register_with_old() {
    #[cfg(not(windows))]
    {
        let mut new_act: libc::sigaction = unsafe { std::mem::zeroed() };
        new_act.sa_sigaction = test_handler as usize;
        unsafe {
            libc::sigaction(libc::SIGINT, &new_act, std::ptr::null_mut());
        }
    }

    #[cfg(windows)]
    {
        unsafe {
            libc::signal(libc::SIGINT, test_handler as usize);
        }
    }

    let res = unsafe {
        ylong_signal::register_signal_action(libc::SIGINT, move || {
            let global = Global::get_instance();
            assert_eq!(global.value.load(Ordering::Relaxed), 0);
            global.value.fetch_add(2, Ordering::Relaxed);
        })
    };
    assert!(res.is_ok());

    unsafe {
        libc::raise(libc::SIGINT);
    }

    let global = Global::get_instance();
    assert_eq!(global.value.load(Ordering::Relaxed), 2);

    let res = ylong_signal::deregister_signal_action(libc::SIGINT);
    assert!(res.is_ok());

    unsafe {
        libc::raise(libc::SIGINT);
    }
    assert_eq!(global.value.load(Ordering::Relaxed), 3);
    let res = ylong_signal::deregister_signal_hook(libc::SIGINT);
    assert!(res.is_ok());
}

pub struct Global {
    value: AtomicUsize,
}

impl Global {
    fn get_instance() -> &'static Global {
        static mut GLOBAL: MaybeUninit<Global> = MaybeUninit::uninit();
        static ONCE: Once = Once::new();

        unsafe {
            ONCE.call_once(|| {
                GLOBAL = MaybeUninit::new(Global {
                    value: AtomicUsize::new(0),
                });
            });
            &*GLOBAL.as_ptr()
        }
    }
}

extern "C" fn test_handler(_sig_num: c_int) {
    let global = Global::get_instance();
    global.value.fetch_add(1, Ordering::Relaxed);
}

/// SDV cases for signal register in multi-thread env
///
/// # Brief
/// 1. Registers a signal handler
/// 2. Spawns another thread to raise the signal
/// 3. Raises the same signal on the main thread
/// 4. All execution should return OK
#[cfg(not(windows))]
fn sdv_signal_register_multi() {
    for i in 0..1000 {
        let res = unsafe {
            ylong_signal::register_signal_action(libc::SIGCHLD, move || {
                let mut data = 100;
                data += i;
                assert_eq!(data, 100 + i);
            })
        };
        std::thread::spawn(move || {
            unsafe { libc::raise(libc::SIGCHLD) };
        });
        assert!(res.is_ok());
        unsafe {
            libc::raise(libc::SIGCHLD);
        }

        let res = ylong_signal::deregister_signal_action(libc::SIGCHLD);
        assert!(res.is_ok());

        unsafe {
            libc::raise(libc::SIGCHLD);
        }
    }
}

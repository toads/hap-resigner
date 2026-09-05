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

#![cfg(feature = "macros")]

/// SDV test cases for select! basic usage case
///
/// # Brief
/// 1. Uses select! to run three async task.
/// 2. Uses if to disabled do_async1() and do_async3().
/// 3. Only the do_async2() task will be completely first.
#[test]
fn sdv_new_select_basic() {
    async fn do_async1() -> i32 {
        1
    }

    async fn do_async2() -> i32 {
        2
    }

    async fn do_async3() -> bool {
        false
    }

    let handle = ylong_runtime::spawn(async {
        let mut count = 0;
        ylong_runtime::select! {
            a = do_async1(), if false => {
                count += a;
            },
            b = do_async2() => {
                count += b;
            },
            c = do_async3(), if false => {
                if c {
                    count = 3;
                }
                else {
                    count = 4;
                }
            }
        }
        assert_eq!(count, 2);
    });
    ylong_runtime::block_on(handle).expect("select! fail");
}

/// SDV test cases for select! oneshot::channel usage
///
/// # Brief
/// 1. Creates two oneshot::channel and send message.
/// 2. Repeated uses select! until both channel return.
/// 3. Checks whether the returned information of the two channels is correct.
#[test]
#[cfg(feature = "sync")]
fn sdv_new_select_channel() {
    let handle = ylong_runtime::spawn(async {
        let (tx1, mut rx1) = ylong_runtime::sync::oneshot::channel();
        let (tx2, mut rx2) = ylong_runtime::sync::oneshot::channel();

        ylong_runtime::spawn(async move {
            tx1.send("first").unwrap();
        });

        ylong_runtime::spawn(async move {
            tx2.send("second").unwrap();
        });

        let mut a = None;
        let mut b = None;

        while a.is_none() || b.is_none() {
            ylong_runtime::select! {
                v1 = (&mut rx1), if a.is_none() => a = Some(v1.unwrap()),
                v2 = (&mut rx2), if b.is_none() => b = Some(v2.unwrap()),
            }
        }

        let res = (a.unwrap(), b.unwrap());

        assert_eq!(res.0, "first");
        assert_eq!(res.1, "second");
    });
    ylong_runtime::block_on(handle).expect("select! fail");
}

/// SDV test cases for select! 'biased' usage
///
/// # Brief
/// 1. Uses 'biased' to execute four task in the specified sequence.
/// 2. Checks whether the 'count' is correct.
#[test]
fn sdv_new_select_biased() {
    let handle = ylong_runtime::spawn(async {
        let mut count = 0u8;

        loop {
            ylong_runtime::select! {
                biased;
                _ = async {}, if count < 1 => {
                    count += 1;
                    assert_eq!(count, 1);
                }
                _ = async {}, if count < 2 => {
                    count += 1;
                    assert_eq!(count, 2);
                }
                _ = async {}, if count < 3 => {
                    count += 1;
                    assert_eq!(count, 3);
                }
                _ = async {}, if count < 4 => {
                    count += 1;
                    assert_eq!(count, 4);
                }
                else => {
                    break;
                }
            }
        }

        assert_eq!(count, 4);
    });
    ylong_runtime::block_on(handle).expect("select! fail");
}

/// SDV test cases for select! match usage
///
/// # Brief
/// 1. Uses select! to run three async task.
/// 2. do_async2() and do_async3() will never match success.
/// 3. Only the do_async1() task will be completely.
#[test]
fn sdv_new_select_match() {
    async fn do_async1() -> i32 {
        1
    }

    async fn do_async2() -> Option<i32> {
        Some(2)
    }

    async fn do_async3() -> Option<bool> {
        None
    }

    let handle = ylong_runtime::spawn(async {
        let mut count = 0;
        ylong_runtime::select! {
            a = do_async1() => {
                count += a;
            },
            None = do_async2() => {
                count += 2;
            },
            Some(c) = do_async3() => {
                if c {
                    count = 3;
                }
                else {
                    count = 4;
                }
            }
        }
        assert_eq!(count, 1);
    });
    ylong_runtime::block_on(handle).expect("select! fail");
}

/// SDV test cases for select! precondition usage
///
/// # Brief
/// 1. Creates a struct and implement a call to the `&mut self` async fn.
/// 2. Uses select! to run the async fn, and sets the precondition to the struct
///    member variable.
/// 3. The select! will be successfully executed.
#[test]
fn sdv_new_select_precondition() {
    struct TestStruct {
        bool: bool,
    }
    impl TestStruct {
        async fn do_async(&mut self) {}
    }

    let handle = ylong_runtime::spawn(async {
        let mut count = 0;
        let mut test_struct = TestStruct { bool: true };
        ylong_runtime::select! {
            _ = test_struct.do_async(), if test_struct.bool => {
                count += 1;
            },
        }
        assert_eq!(count, 1);
    });
    ylong_runtime::block_on(handle).expect("select! fail");
}

/// SDV test cases for select! panic! usage
///
/// # Brief
/// 1. Uses select! to run two async task with `if false`.
/// 2. All branches will be disabled and select! will be panic!
#[test]
#[should_panic]
fn sdv_new_select_panic() {
    async fn do_async1() {}

    async fn do_async2() {}

    let handle = ylong_runtime::spawn(async {
        ylong_runtime::select! {
            _ = do_async1(), if false => {},
            _ = do_async2(), if false => {}
        }
    });
    ylong_runtime::block_on(handle).expect("select! fail");
}

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

/// Waits on multiple concurrent branches, returning when the **first** branch
/// completes, cancelling the remaining branches.
///
/// There is **no upper limit** on the number of branches.
///
/// The `select!` macro must be used inside of async functions, closures, and
/// blocks.
///
/// Branchex must comply with the following format:
///
/// ```text
/// <pattern> = <async expression> (, if <precondition>)? => <handler>,
/// ```
///
/// The `<async expression>` for all branches will run concurrently. Once the
/// first expression is completed, the corresponding `<handler>` will be
/// executed. Each branch is optional `if <precondition>`. if the
/// `<precondition>` returns false, the corresponding `<handler>` will not be
/// executed.
///
/// When none of the branches can match, Executes the else expression.
///
/// ```text
/// else => <expression>
/// ```
///
/// # Examples
///
/// Basic select with two branches.
/// ```
/// async fn do_async1() {
///     // do task
/// }
/// async fn do_async2() {
///     // do task
/// }
/// async fn select_test() {
///     ylong_runtime::select! {
///         _ = do_async1() => {
///             println!("do_async1() completed first");
///         },
///         _ = do_async2() => {
///             println!("do_async2() completed first");
///         }
///     }
/// }
/// ```
///
/// # Examples
///
/// Uses if to filter asynchronous tasks
/// ```
/// async fn do_async1() -> i32 {
///     1
/// }
/// async fn do_async2() -> i32 {
///     2
/// }
/// async fn do_async3() -> bool {
///     false
/// }
/// async fn select_test() {
///     let mut count = 0;
///     ylong_runtime::select! {
///         a = do_async1(), if false => {
///             count += a;
///             println!("do_async1() completed first{:?}", a);
///         },
///         b = do_async2() => {
///             count += b;
///             println!("do_async2() completed first{:?}", b);
///         },
///         c = do_async3(), if false => {
///             if c {
///                 println!("do_async3() completed true");
///             }
///             else {
///                 println!("do_async3() completed false");
///             }
///         }
///     }
///     assert_eq!(count, 2);
/// }
/// ```
/// # Examples
///
/// Repeated uses select! until all task return.
/// ```
/// #[cfg(feature = "sync")]
/// async fn select_channel() {
///     let (tx1, mut rx1) = ylong_runtime::sync::oneshot::channel();
///     let (tx2, mut rx2) = ylong_runtime::sync::oneshot::channel();
///
///     ylong_runtime::spawn(async move {
///         tx1.send("first").unwrap();
///     });
///
///     ylong_runtime::spawn(async move {
///         tx2.send("second").unwrap();
///     });
///
///     let mut a = None;
///     let mut b = None;
///
///     while a.is_none() || b.is_none() {
///         ylong_runtime::select! {
///             v1 = (&mut rx1), if a.is_none() => a = Some(v1.unwrap()),
///             v2 = (&mut rx2), if b.is_none() => b = Some(v2.unwrap()),
///         }
///     }
///
///     let res = (a.unwrap(), b.unwrap());
///
///     assert_eq!(res.0, "first");
///     assert_eq!(res.1, "second");
/// }
/// ```
/// # Examples
///
/// Uses 'biased' to execute four task in the specified sequence.
/// It will poll branches with order, the first branch will be polled first.
/// ```
/// async fn select_biased() {
///     let mut count = 0u8;
///
///     loop {
///         ylong_runtime::select! {
///             biased;
///             _ = async {}, if count < 1 => {
///                 count += 1;
///                 assert_eq!(count, 1);
///             }
///             _ = async {}, if count < 2 => {
///                 count += 1;
///                 assert_eq!(count, 2);
///             }
///             _ = async {}, if count < 3 => {
///                 count += 1;
///                 assert_eq!(count, 3);
///             }
///             _ = async {}, if count < 4 => {
///                 count += 1;
///                 assert_eq!(count, 4);
///             }
///             else => {
///                 break;
///             }
///         }
///     }
///
///     assert_eq!(count, 4);
/// }
/// ```
#[macro_export]
macro_rules! select {
    ( {
        // Branch from which the execution starts.
        random = $bool:expr;
        // Branch count.
        ( $count:expr, $($_n:tt)* )
        // ( index:expr ) Branch's index
        $( ( $index:expr, $($_i:tt)* ) $bind:pat = $fut:expr, if $c:expr => $handle:expr, )+
        // When all branches fail, executes this;
        ; $else:expr

    }) => {{
        use std::future::Future;
        use std::pin::Pin;
        use std::task::Poll::{Ready, Pending};

        enum Out<T> {
            Finish(T),
            Fail,
        }

        let branches_count: usize = $count;

        // The ith element indicates whether the ith branch is available.
        let mut match_result = [true; $count];
        // Handle preconditions, this step cannot be handled within poll_fn()
        $(
            if (!$c)
            {
                match_result[$index] = false;
            }
        )*

        // When a branch ready first, modify this variable to
        // branch's index to ensure that the branch is executed first.
        use $crate::fastrand::fast_random;
        let mut random_number = fast_random() as usize;

        let output = {
            let mut futures = ( $( $fut , )+ );
            let futures = &mut futures;

            $crate::futures::poll_fn(|cx| {
                let mut anyone_pending = false;
                let random = $bool;

                for i in 0..branches_count {
                    let branch  = match random {
                        true => {(random_number + i) % branches_count },
                        false => i
                    };

                    $(
                        if (branch == $index && match_result[branch])
                        {
                            let ( $($_i,)* fut, .. ) = &mut *futures;
                            let fut = unsafe { Pin::new_unchecked(fut) };
                            match Future::poll(fut, cx) {
                                Ready(out) => {
                                    // Check if the returned value match the user input.
                                    match &out {
                                        // If the match is successful, the inner value will
                                        // never used, so here has to use unused_variables.
                                        #[allow(unused_variables)]
                                        $bind => {
                                            // Change the random_number, ensure when return ready, this branch is executed first.
                                            random_number = branch;
                                            return Ready($crate::tuple_form!(($count) with Out::Fail except Out::Finish(out) at ($($_i)*)))
                                        },
                                        // If the match fails, this branch set false, and wait for the next branch to complete.
                                        // When user input is not match type, this patterns is unreachable.
                                        #[allow(unreachable_patterns)]
                                        _ => {
                                            match_result[branch] = false;
                                            continue;
                                        }
                                    }
                                },
                                Pending => {
                                    anyone_pending = true;
                                    continue;
                                }
                            };
                        }
                    )*
                }

                if anyone_pending {
                    Pending
                } else {
                    Ready($crate::tuple_form!(($count) with Out::Fail except Out::Fail at ($($_n)*)))
                }
            }).await
        };

        match output {
            $(
                $crate::tuple_form!(($count) with Out::Fail except Out::Finish($bind) at ($($_i)*)) => $handle,
            )*
            $crate::tuple_form!(($count) with Out::Fail except Out::Fail at ($($_n)*)) => $else,
            // If there is only one branch and the user match for that branch returns `_`,
            // there will be an unreachable pattern alert.
            #[allow(unreachable_patterns)]
            _ => unreachable!("finally match fail"),
        }
    }};

    // if there is no 'else' branch, add the default 'else' branch.
    ( { random = $bool:expr; $($t:tt)* } ) => {
        $crate::select!({ random = $bool; $($t)*; panic!("select!: All the branches failed.") })
    };
    // if there is an 'else' branch, add the 'else' branch into {}.
    ( { random = $bool:expr; $($t:tt)* } else => $else:expr $(,)?) => {
        $crate::select!({ random = $bool; $($t)*; $else })
    };
    // Recursively join a branch into {}.
    // The branch is separated by ',', has 'if' conditions and executes block finally.
    ( { random = $bool:expr; ( $s:expr, $($_n:tt)* ) $($t:tt)* } $p:pat = $f:expr, if $c:expr => $h:block, $($r:tt)* ) => {
        $crate::select!({ random = $bool; ( $s + 1, $($_n)*_) $($t)* ($s, $($_n)*) $p = $f, if $c => $h, } $($r)*)
    };
    // Recursively join a branch into {}.
    // The branch is separated by ',', does not has 'if' conditions and executes block finally.
    ( { random = $bool:expr; ( $s:expr, $($_n:tt)* ) $($t:tt)* } $p:pat = $f:expr => $h:block, $($r:tt)* ) => {
        $crate::select!({ random = $bool; ( $s + 1, $($_n)*_) $($t)* ($s, $($_n)*) $p = $f, if true => $h, } $($r)*)
    };
    // Recursively join a branch into {}.
    // The branch is separated by ' ', has 'if' conditions and executes block finally.
    ( { random = $bool:expr; ( $s:expr, $($_n:tt)* ) $($t:tt)* } $p:pat = $f:expr, if $c:expr => $h:block $($r:tt)* ) => {
        $crate::select!({ random = $bool; ( $s + 1, $($_n)*_) $($t)* ($s, $($_n)*) $p = $f, if $c => $h, } $($r)*)
    };
    // Recursively join a branch into {}.
    // The branch is separated by ' ', does not has 'if' conditions and executes block finally.
    ( { random = $bool:expr; ( $s:expr, $($_n:tt)* ) $($t:tt)* } $p:pat = $f:expr => $h:block $($r:tt)* ) => {
        $crate::select!( { random = $bool; ( $s + 1, $($_n)*_) $($t)* ($s, $($_n)*) $p = $f, if true => $h, } $($r)*)
    };
    // Recursively join a branch into {}.
    // The branch is separated by ',', has 'if' conditions and executes expressions finally.
    ( { random = $bool:expr; ( $s:expr, $($_n:tt)*  ) $($t:tt)* } $p:pat = $f:expr, if $c:expr => $h:expr, $($r:tt)* ) => {
        $crate::select!({ random = $bool; ( $s + 1, $($_n)*_ ) $($t)* ($s, $($_n)*) $p = $f, if $c => $h, } $($r)*)
    };
    // Recursively join a branch into {}.
    // The branch is separated by ',', does not has 'if' conditions and executes expressions finally.
    ( { random = $bool:expr; ( $s:expr, $($_n:tt)*  ) $($t:tt)* } $p:pat = $f:expr => $h:expr, $($r:tt)* ) => {
        $crate::select!({ random = $bool; ( $s + 1, $($_n)*_ ) $($t)* ($s, $($_n)*) $p = $f, if true => $h, } $($r)*)
    };
    // Recursively join the last branch into {}.
    // The branch is separated by ',', has 'if' conditions and executes expressions finally.
    // If the branch executes expressions finally, it can't separated by ' '.
    ( { random = $bool:expr; ( $s:expr, $($_n:tt)*  ) $($t:tt)* } $p:pat = $f:expr, if $c:expr => $h:expr ) => {
        $crate::select!({ random = $bool; ( $s + 1, $($_n)*_ ) $($t)* ($s, $($_n)*) $p = $f, if $c => $h, })
    };
    // Recursively join the last branch into {}.
    // The branch is separated by ',', does not has 'if' conditions and executes expressions finally.
    // If the branch executes expressions finally, it can't separated by ' '.
    ( { random = $bool:expr; ( $s:expr, $($_n:tt)*  ) $($t:tt)* } $p:pat = $f:expr => $h:expr ) => {
        $crate::select!({ random = $bool; ( $s + 1, $($_n)*_ ) $($t)* ($s, $($_n)*) $p = $f, if true => $h, })
    };

    // Usage entry. Starts with the first branch.
    (biased; $p:pat = $($t:tt)* ) => {
        $crate::select!({ random = false; ( 0,) } $p = $($t)*)
    };
    // Usage entry. Starts with random branch.
    ( $p:pat = $($t:tt)* ) => {
        $crate::select!({ random = true; ( 0,) } $p = $($t)*)
    };
    // There is no branch.
    () => {
        compile_error!("select! requires at least one branch.")
    };
}

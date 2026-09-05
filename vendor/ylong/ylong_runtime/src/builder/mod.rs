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

//! A builder to configure the runtime, and thread pool of the runtime.
//!
//! Ylong-runtime provides two kinds of runtime.
//! `CurrentThread`: Runtime which runs on the current thread.
//! `MultiThread`: Runtime which runs on multiple threads.
//!
//! After configuring the builder, a call to `build` will return the actual
//! runtime instance. [`MultiThreadBuilder`] could also be used for configuring
//! the global singleton runtime.
//!
//! For thread pool, the builder allows the user to set the thread number, stack
//! size and name prefix of each thread.

pub(crate) mod common_builder;
#[cfg(feature = "current_thread_runtime")]
pub(crate) mod current_thread_builder;
pub(crate) mod multi_thread_builder;

use std::fmt::Debug;
use std::sync::Arc;

#[cfg(feature = "current_thread_runtime")]
pub use current_thread_builder::CurrentThreadBuilder;
pub use multi_thread_builder::MultiThreadBuilder;

pub(crate) use crate::builder::common_builder::CommonBuilder;

cfg_not_ffrt!(
    use crate::error::ScheduleError;
    use crate::executor::async_pool::AsyncPoolSpawner;
    use crate::executor::blocking_pool::BlockPoolSpawner;
    use std::io;
);

/// A callback function to be executed in different stages of a thread's
/// life-cycle
pub type CallbackHook = Arc<dyn Fn() + Send + Sync + 'static>;

/// Schedule Policy.
#[derive(Debug, Clone, Copy, PartialOrd, PartialEq, Eq)]
pub enum ScheduleAlgo {
    /// Bounded local queues which adopts FIFO order.
    FifoBound,
}

/// Builder to build the runtime. Provides methods to customize the runtime,
/// such as setting thread pool size, worker thread stack size, work thread
/// prefix and etc.
///
/// If `multi_instance_runtime` or `current_thread_runtime` feature is turned
/// on: After setting the RuntimeBuilder, a call to build will initialize the
/// actual runtime and returns its instance. If there is an invalid parameter
/// during the build, an error would be returned.
///
/// Otherwise:
/// RuntimeBuilder will not have the `build()` method, instead, this builder
/// should be passed to set the global executor.
///
/// # Examples
///
/// ```no run
/// #![cfg(feature = "multi_instance_runtime")]
///
/// use ylong_runtime::builder::RuntimeBuilder;
/// use ylong_runtime::executor::Runtime;
///
/// let runtime = RuntimeBuilder::new_multi_thread()
///     .worker_num(4)
///     .worker_stack_size(1024 * 300)
///     .build()
///     .unwrap();
/// ```
pub struct RuntimeBuilder;

impl RuntimeBuilder {
    /// Initializes a new RuntimeBuilder with current_thread settings.
    ///
    /// All tasks will run on the current thread, which means it does not create
    /// any other worker threads.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::builder::RuntimeBuilder;
    ///
    /// let builder = RuntimeBuilder::new_current_thread()
    ///     .worker_stack_size(1024 * 3)
    ///     .max_blocking_pool_size(4);
    /// ```
    #[cfg(feature = "current_thread_runtime")]
    pub fn new_current_thread() -> CurrentThreadBuilder {
        CurrentThreadBuilder::new()
    }

    /// Initializes a new RuntimeBuilder with multi_thread settings.
    ///
    /// When running, worker threads will be created according to the builder
    /// configuration, and tasks will be allocated and run in the newly
    /// created thread pool.
    ///
    /// # Examples
    ///
    /// ```
    /// use ylong_runtime::builder::RuntimeBuilder;
    ///
    /// let builder = RuntimeBuilder::new_multi_thread();
    /// ```
    pub fn new_multi_thread() -> MultiThreadBuilder {
        MultiThreadBuilder::new()
    }
}

cfg_not_ffrt! {
    pub(crate) fn initialize_async_spawner(
        builder: &MultiThreadBuilder,
    ) -> io::Result<AsyncPoolSpawner> {
        AsyncPoolSpawner::new(builder)
    }

    pub(crate) fn initialize_blocking_spawner(
        builder: &CommonBuilder,
    ) -> Result<BlockPoolSpawner, ScheduleError> {
        let blocking_spawner = BlockPoolSpawner::new(builder);
        blocking_spawner.create_permanent_threads()?;
        Ok(blocking_spawner)
    }
}

#[cfg(test)]
mod test {
    use crate::builder::RuntimeBuilder;
    #[cfg(not(feature = "ffrt"))]
    use crate::builder::ScheduleAlgo;

    /// UT test cases for RuntimeBuilder::new_multi_thread()
    ///
    /// # Brief
    /// 1. Checks if the object name property is None
    /// 2. Checks if the object core_pool_size property is None
    /// 3. Checks if the object is_steal property is true
    /// 4. Checks if the object is_affinity property is true
    /// 5. Checks if the object permanent_blocking_thread_num property is 4
    /// 6. Checks if the object max_pool_size property is Some(50)
    /// 7. Checks if the object keep_alive_time property is None
    /// 8. Checks if the object schedule_algo property is
    ///    ScheduleAlgo::FifoBound
    /// 9. Checks if the object stack_size property is None
    /// 10. Checks if the object after_start property is None
    /// 11. Checks if the object before_stop property is None
    #[test]
    fn ut_thread_pool_builder_new() {
        let thread_pool_builder = RuntimeBuilder::new_multi_thread();
        assert_eq!(thread_pool_builder.common.worker_name, None);
        #[cfg(not(feature = "ffrt"))]
        {
            assert_eq!(thread_pool_builder.common.blocking_permanent_thread_num, 0);
            assert_eq!(thread_pool_builder.common.max_blocking_pool_size, None);
            assert_eq!(thread_pool_builder.common.keep_alive_time, None);
            assert_eq!(thread_pool_builder.core_thread_size, None);
            assert_eq!(thread_pool_builder.common.stack_size, None);
            assert_eq!(
                thread_pool_builder.common.schedule_algo,
                ScheduleAlgo::FifoBound
            );
        }
    }

    /// UT test cases for RuntimeBuilder::name()
    ///
    /// # Brief
    /// 1. Checks if the object name property is modified value
    #[test]
    fn ut_thread_pool_builder_name() {
        let name = String::from("worker_name");
        let thread_pool_builder = RuntimeBuilder::new_multi_thread().worker_name(name.clone());
        assert_eq!(thread_pool_builder.common.worker_name, Some(name));
    }

    /// UT test cases for RuntimeBuilder::core_pool_size()
    ///
    /// # Brief
    /// 1. core_pool_size set to 1, Check if the return value is Some(1)
    /// 2. core_pool_size set to 64, Check if the return value is Some(64)
    /// 3. core_pool_size set to 0, Check if the return value is Some(1)
    /// 4. core_pool_size set to 65, Check if the return value is Some(64)
    #[test]
    #[cfg(not(feature = "ffrt"))]
    fn ut_thread_pool_builder_core_pool_size() {
        let thread_pool_builder = RuntimeBuilder::new_multi_thread().worker_num(1);
        assert_eq!(thread_pool_builder.core_thread_size, Some(1));

        let thread_pool_builder = RuntimeBuilder::new_multi_thread().worker_num(64);
        assert_eq!(thread_pool_builder.core_thread_size, Some(64));

        let thread_pool_builder = RuntimeBuilder::new_multi_thread().worker_num(0);
        assert_eq!(thread_pool_builder.core_thread_size, Some(1));

        let thread_pool_builder = RuntimeBuilder::new_multi_thread().worker_num(65);
        assert_eq!(thread_pool_builder.core_thread_size, Some(64));
    }

    /// UT test cases for RuntimeBuilder::stack_size()
    ///
    /// # Brief
    /// 1. stack_size set to 0, Check if the return value is Some(1)
    /// 2. stack_size set to 1, Check if the return value is Some(1)
    #[test]
    #[cfg(not(feature = "ffrt"))]
    fn ut_thread_pool_builder_stack_size() {
        let thread_pool_builder = RuntimeBuilder::new_multi_thread().worker_stack_size(0);
        assert_eq!(thread_pool_builder.common.stack_size.unwrap(), 1);

        let thread_pool_builder = RuntimeBuilder::new_multi_thread().worker_stack_size(1);
        assert_eq!(thread_pool_builder.common.stack_size.unwrap(), 1);
    }
}

#[cfg(test)]
#[cfg(feature = "current_thread_runtime")]
mod current_thread_test {
    use crate::builder::RuntimeBuilder;

    /// UT test cases for new_current_thread.
    ///
    /// # Brief
    /// 1. Verify the result when multiple tasks are inserted to the current
    ///    thread at a time.
    /// 2. Insert the task for multiple times, wait until the task is complete,
    ///    verify the result, and then perform the operation again.
    /// 3. Spawn nest thread.
    #[test]
    fn ut_thread_pool_builder_current_thread() {
        let runtime = RuntimeBuilder::new_current_thread().build().unwrap();
        let mut handles = vec![];
        for index in 0..1000 {
            let handle = runtime.spawn(async move { index });
            handles.push(handle);
        }
        for (index, handle) in handles.into_iter().enumerate() {
            let result = runtime.block_on(handle).unwrap();
            assert_eq!(result, index);
        }

        let runtime = RuntimeBuilder::new_current_thread().build().unwrap();
        for index in 0..1000 {
            let handle = runtime.spawn(async move { index });
            let result = runtime.block_on(handle).unwrap();
            assert_eq!(result, index);
        }

        let runtime = RuntimeBuilder::new_current_thread().build().unwrap();
        let handle = runtime.spawn_blocking(|| {
            let runtime = RuntimeBuilder::new_current_thread().build().unwrap();
            let handle = runtime.spawn(async move { 1_usize });
            let result = runtime.block_on(handle).unwrap();
            assert_eq!(result, 1);
            result
        });
        let result = runtime.block_on(handle).unwrap();
        assert_eq!(result, 1);
    }
}

#[cfg(not(feature = "ffrt"))]
#[cfg(test)]
mod ylong_executor_test {
    use crate::builder::{RuntimeBuilder, ScheduleAlgo};
    use crate::util::num_cpus::get_cpu_num;

    /// UT test cases for ThreadPoolBuilder::is_affinity()
    ///
    /// # Brief
    /// 1. is_affinity set to true, check if it is a modified value
    /// 2. is_affinity set to false, check if it is a modified value
    #[test]
    fn ut_thread_pool_builder_is_affinity() {
        let thread_pool_builder = RuntimeBuilder::new_multi_thread().is_affinity(true);
        assert!(thread_pool_builder.common.is_affinity);

        let thread_pool_builder = RuntimeBuilder::new_multi_thread().is_affinity(false);
        assert!(!thread_pool_builder.common.is_affinity);
    }

    /// UT test cases for RuntimeBuilder::blocking_permanent_thread_num()
    ///
    /// # Brief        
    /// 1. permanent_blocking_thread_num set to 1, check if the return value is
    ///    1.
    /// 2. permanent_blocking_thread_num set to max_thread_num, check if the
    ///    return value is max_blocking_pool_size.
    /// 3. permanent_blocking_thread_num set to 0, check if the return value is
    ///    1.
    /// 4. permanent_blocking_thread_num set to max_thread_num + 1, Check if the
    ///    return value O is max_blocking_pool_size.
    #[test]
    fn ut_thread_pool_builder_permanent_blocking_thread_num() {
        let thread_pool_builder =
            RuntimeBuilder::new_multi_thread().blocking_permanent_thread_num(1);
        assert_eq!(thread_pool_builder.common.blocking_permanent_thread_num, 1);

        let blocking_permanent_thread_num = get_cpu_num() as u8;
        let thread_pool_builder = RuntimeBuilder::new_multi_thread()
            .blocking_permanent_thread_num(blocking_permanent_thread_num);
        assert_eq!(
            thread_pool_builder.common.blocking_permanent_thread_num,
            blocking_permanent_thread_num
        );

        let thread_pool_builder =
            RuntimeBuilder::new_multi_thread().blocking_permanent_thread_num(0);
        assert_eq!(thread_pool_builder.common.blocking_permanent_thread_num, 0);

        let permanent_blocking_thread_num = get_cpu_num() as u8 + 1;
        let thread_pool_builder = RuntimeBuilder::new_multi_thread()
            .blocking_permanent_thread_num(permanent_blocking_thread_num);
        assert_eq!(
            thread_pool_builder.common.blocking_permanent_thread_num,
            permanent_blocking_thread_num
        );
    }

    /// UT test cases for RuntimeBuilder::max_pool_size()
    ///
    /// # Brief
    /// 1. max_pool_size set to 1, check if the return value is Some(1)
    /// 2. max_pool_size set to 64, check if the return value is Some(64)
    /// 3. max_pool_size set to 0, check if the return value is Some(1)
    /// 4. max_pool_size set to 65, check if the return value is Some(64)
    #[test]
    fn ut_thread_pool_builder_max_pool_size() {
        let thread_pool_builder = RuntimeBuilder::new_multi_thread().max_blocking_pool_size(1);
        assert_eq!(
            thread_pool_builder.common.max_blocking_pool_size.unwrap(),
            1
        );

        let thread_pool_builder = RuntimeBuilder::new_multi_thread().max_blocking_pool_size(64);
        assert_eq!(
            thread_pool_builder.common.max_blocking_pool_size.unwrap(),
            64
        );

        let thread_pool_builder = RuntimeBuilder::new_multi_thread().max_blocking_pool_size(0);
        assert_eq!(
            thread_pool_builder.common.max_blocking_pool_size.unwrap(),
            1
        );

        let thread_pool_builder = RuntimeBuilder::new_multi_thread().max_blocking_pool_size(65);
        assert_eq!(
            thread_pool_builder.common.max_blocking_pool_size.unwrap(),
            64
        );
    }

    /// UT test cases for RuntimeBuilder::keep_alive_time()
    ///
    /// # Brief
    /// 1. keep_alive_time set to 0, check if the return value is
    ///    Some(Duration::from_secs(0))
    /// 2. keep_alive_time set to 1, check if the return value is
    ///    Some(Duration::from_secs(1))
    #[test]
    fn ut_thread_pool_builder_keep_alive_time() {
        use std::time::Duration;

        let keep_alive_time = Duration::from_secs(0);
        let thread_pool_builder =
            RuntimeBuilder::new_multi_thread().keep_alive_time(keep_alive_time);
        assert_eq!(
            thread_pool_builder.common.keep_alive_time.unwrap(),
            keep_alive_time
        );

        let keep_alive_time = Duration::from_secs(1);
        let thread_pool_builder =
            RuntimeBuilder::new_multi_thread().keep_alive_time(keep_alive_time);
        assert_eq!(
            thread_pool_builder.common.keep_alive_time.unwrap(),
            keep_alive_time
        );
    }

    /// UT test cases for RuntimeBuilder::schedule_algo()
    ///
    /// # Brief
    /// 1. schedule_algo set to FifoBound, check if it is the modified value
    #[cfg(not(feature = "ffrt"))]
    #[test]
    fn ut_thread_pool_builder_schedule_algo_test() {
        let schedule_algo = ScheduleAlgo::FifoBound;
        let thread_pool_builder = RuntimeBuilder::new_multi_thread().schedule_algo(schedule_algo);
        assert_eq!(thread_pool_builder.common.schedule_algo, schedule_algo);
    }
}

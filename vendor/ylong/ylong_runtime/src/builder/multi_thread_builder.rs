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
use std::sync::Mutex;

cfg_ffrt!(
    use ylong_ffrt::{ffrt_set_worker_stack_size, Qos};
    use std::collections::HashMap;
    use libc::{c_uint, c_ulong};
    use std::time::Duration;
    use crate::builder::ScheduleAlgo;
);

#[cfg(not(feature = "ffrt"))]
use crate::builder::common_builder::impl_common;
use crate::builder::CommonBuilder;
#[cfg(feature = "multi_instance_runtime")]
use crate::executor::{AsyncHandle, Runtime};

pub(crate) static GLOBAL_BUILDER: Mutex<Option<MultiThreadBuilder>> = Mutex::new(None);

/// Runtime builder that configures a multi-threaded runtime, or the global
/// runtime.
pub struct MultiThreadBuilder {
    pub(crate) common: CommonBuilder,

    #[cfg(not(feature = "ffrt"))]
    /// Maximum thread number for core thread pool
    pub(crate) core_thread_size: Option<usize>,

    #[cfg(feature = "ffrt")]
    /// Thread number for each qos
    pub(crate) thread_num_by_qos: HashMap<Qos, u32>,
}

impl MultiThreadBuilder {
    pub(crate) fn new() -> Self {
        MultiThreadBuilder {
            common: CommonBuilder::new(),
            #[cfg(not(feature = "ffrt"))]
            core_thread_size: None,
            #[cfg(feature = "ffrt")]
            thread_num_by_qos: HashMap::new(),
        }
    }

    /// Configures the global runtime.
    ///
    /// # Error
    /// If the global runtime is already running or this method has been called
    /// before, then it will return an `AlreadyExists` error.
    pub fn build_global(self) -> io::Result<()> {
        let mut builder = GLOBAL_BUILDER.lock().unwrap();
        if builder.is_some() {
            return Err(io::ErrorKind::AlreadyExists.into());
        }

        #[cfg(feature = "ffrt")]
        unsafe {
            for (qos, stack_size) in self.common.stack_size_by_qos.iter() {
                ffrt_set_worker_stack_size(*qos, *stack_size as c_ulong);
            }
        }

        *builder = Some(self);
        Ok(())
    }
}

#[cfg(feature = "ffrt")]
impl MultiThreadBuilder {
    /// Sets the maximum worker number for a specific qos group.
    ///
    /// If a worker number has already been set for a qos, calling the method
    /// with the same qos will overwrite the old value.
    ///
    /// # Error
    /// The accepted worker number range for each qos is [1, 20]. If 0 is passed
    /// in, then the maximum worker number will be set to 1. If a number
    /// greater than 20 is passed in, then the maximum worker number will be
    /// set to 20.
    pub fn max_worker_num_by_qos(mut self, qos: Qos, num: u32) -> Self {
        let worker = match num {
            0 => 1,
            n if n > 20 => 20,
            n => n,
        };
        self.thread_num_by_qos.insert(qos, worker);
        self
    }

    /// Sets the name prefix for all worker threads.
    pub fn worker_name(mut self, name: String) -> Self {
        self.common.worker_name = Some(name);
        self
    }

    /// Sets the number of core worker threads.
    ///
    ///
    /// The boundary of thread number is 1-64:
    /// If sets a number smaller than 1, then thread number would be set to 1.
    /// If sets a number larger than 64, then thread number would be set to 64.
    /// The default value is the number of cores of the cpu.
    ///
    /// # Examples
    /// ```
    /// use crate::ylong_runtime::builder::RuntimeBuilder;
    ///
    /// let runtime = RuntimeBuilder::new_multi_thread().worker_num(8);
    /// ```
    pub fn worker_num(self, core_pool_size: usize) -> Self {
        self.max_worker_num_by_qos(Qos::Default, core_pool_size as u32)
    }

    /// Sets the core affinity of the worker threads
    ///
    /// # Note
    /// This method does nothing now under ffrt feature.
    pub fn is_affinity(self, _is_affinity: bool) -> Self {
        self
    }

    /// Sets the schedule policy.
    ///
    /// # Note
    /// This method does nothing now under ffrt feature.
    pub fn schedule_algo(self, _schedule_algo: ScheduleAlgo) -> Self {
        self
    }

    /// Sets the callback function to be called when a worker thread starts.
    ///
    /// # Note
    /// This method does nothing now under ffrt feature.
    pub fn after_start<F>(self, _f: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self
    }

    /// Sets the callback function to be called when a worker thread stops.
    ///
    /// # Note
    /// This method does nothing now under ffrt feature.
    pub fn before_stop<F>(self, _f: F) -> Self
    where
        F: Fn() + Send + Sync + 'static,
    {
        self
    }

    /// Sets the maximum number of permanent threads in blocking thread pool
    ///
    /// # Note
    /// This method does nothing now under ffrt feature.
    pub fn blocking_permanent_thread_num(self, _blocking_permanent_thread_num: u8) -> Self {
        self
    }

    /// Sets the number of threads that the runtime could spawn additionally
    /// besides the core thread pool.
    ///
    /// The boundary is 1-64.
    ///
    /// # Note
    /// This method does nothing now under ffrt feature.
    pub fn max_blocking_pool_size(self, _max_blocking_pool_size: u8) -> Self {
        self
    }

    /// Sets how long will the thread be kept alive inside the blocking pool
    /// after it becomes idle.
    ///
    /// # Note
    /// This method does nothing now under ffrt feature.
    pub fn keep_alive_time(self, _keep_alive_time: Duration) -> Self {
        self
    }

    /// Sets the thread stack size for a specific qos group.
    ///
    /// If a stack size has already been set for a qos, calling the method
    /// with the same qos will overwrite the old value
    ///
    /// # Error
    /// The lowest accepted stack size is 16k. If a value under 16k is passed
    /// in, then the stack size will be set to 16k instead.
    pub fn stack_size_by_qos(mut self, qos: Qos, stack_size: usize) -> Self {
        const PTHREAD_STACK_MIN: usize = 16 * 1000;

        let stack_size = match stack_size {
            n if n < PTHREAD_STACK_MIN => PTHREAD_STACK_MIN,
            n => n,
        };
        self.common.stack_size_by_qos.insert(qos, stack_size);
        self
    }

    /// Sets the stack size for every worker thread that gets spawned by the
    /// runtime. The minimum stack size is 1.
    pub fn worker_stack_size(self, stack_size: usize) -> Self {
        self.stack_size_by_qos(Qos::Default, stack_size)
    }
}

#[cfg(not(feature = "ffrt"))]
impl MultiThreadBuilder {
    /// Initializes the runtime and returns its instance.
    #[cfg(feature = "multi_instance_runtime")]
    pub fn build(&mut self) -> io::Result<Runtime> {
        use crate::builder::initialize_async_spawner;
        let async_spawner = initialize_async_spawner(self)?;

        Ok(Runtime {
            async_spawner: AsyncHandle::MultiThread(async_spawner),
        })
    }

    /// Sets the number of core worker threads.
    ///
    ///
    /// The boundary of thread number is 1-64:
    /// If sets a number smaller than 1, then thread number would be set to 1.
    /// If sets a number larger than 64, then thread number would be set to 64.
    /// The default value is the number of cores of the cpu.
    ///
    /// # Examples
    /// ```
    /// use crate::ylong_runtime::builder::RuntimeBuilder;
    ///
    /// let runtime = RuntimeBuilder::new_multi_thread().worker_num(8);
    /// ```
    pub fn worker_num(mut self, core_pool_size: usize) -> Self {
        if core_pool_size < 1 {
            self.core_thread_size = Some(1);
        } else if core_pool_size > 64 {
            self.core_thread_size = Some(64);
        } else {
            self.core_thread_size = Some(core_pool_size);
        }
        self
    }
}

#[cfg(not(feature = "ffrt"))]
impl_common!(MultiThreadBuilder);

#[cfg(feature = "full")]
#[cfg(test)]
mod test {
    use crate::builder::RuntimeBuilder;
    use crate::executor::{global_default_async, AsyncHandle};

    /// UT test cases for blocking on a time sleep without initializing the
    /// runtime.
    ///
    /// # Brief
    /// 1. Configure the global runtime to make it have six core threads
    /// 2. Get the global runtime
    /// 3. Check the core thread number of the runtime
    /// 4. Call build_global once more
    /// 5. Check the error
    #[test]
    fn ut_build_global() {
        let ret = RuntimeBuilder::new_multi_thread()
            .worker_num(6)
            .max_blocking_pool_size(3)
            .build_global();
        assert!(ret.is_ok());

        let async_pool = global_default_async();
        match &async_pool.async_spawner {
            AsyncHandle::CurrentThread(_) => unreachable!(),
            AsyncHandle::MultiThread(x) => {
                assert_eq!(x.inner.total, 6);
            }
        }

        let ret = RuntimeBuilder::new_multi_thread()
            .worker_num(2)
            .max_blocking_pool_size(3)
            .build_global();
        assert!(ret.is_err());
    }
}

#[cfg(feature = "ffrt")]
#[cfg(test)]
mod ffrt_test {
    use ylong_ffrt::Qos::{Default, UserInitiated, UserInteractive};

    use crate::builder::MultiThreadBuilder;

    /// UT test cases for max_worker_num_by_qos
    ///
    /// # Brief
    /// 1. Sets UserInteractive qos group to have 0 maximum worker number.
    /// 2. Checks if the actual value is 1
    /// 3. Sets UserInteractive qos group to have 21 maximum worker number.
    /// 4. Checks if the actual value is 20
    /// 5. Set Default qos group to have 8 maximum worker number.
    /// 6. Checks if the actual value is 8.
    /// 7. Calls build_global on the builder, check if the return value is Ok
    #[test]
    fn ut_set_max_worker() {
        let builder = MultiThreadBuilder::new();
        let builder = builder.max_worker_num_by_qos(UserInteractive, 0);
        let num = builder.thread_num_by_qos.get(&UserInteractive).unwrap();
        assert_eq!(*num, 1);

        let builder = builder.max_worker_num_by_qos(UserInteractive, 21);
        let num = builder.thread_num_by_qos.get(&UserInteractive).unwrap();
        assert_eq!(*num, 20);

        let builder = MultiThreadBuilder::new().max_worker_num_by_qos(Default, 8);
        let num = builder.thread_num_by_qos.get(&Default).unwrap();
        assert_eq!(*num, 8);
    }

    /// UT cases for stack_size_by_qos
    ///
    /// # Brief
    /// 1. Sets UserInitiated qos group's stack size to 16k - 1
    /// 2. Checks if the actual stack size is 16k
    /// 3. Sets UserInteractive qos group's stack size to 16k
    /// 4. Checks if the actual stack size is 16k
    /// 5. Sets Default qos group's stack size to 16M
    /// 6. Checks if the actual stack size is 16M
    /// 7. Sets UserInteractive qos group's stack size to 16k + 1
    /// 8. Checks if the actual stack size is 16k + 1
    #[test]
    fn ut_set_stack_size() {
        let builder = MultiThreadBuilder::new();
        let builder = builder.stack_size_by_qos(UserInitiated, 16 * 1000 - 1);
        let num = builder
            .common
            .stack_size_by_qos
            .get(&UserInitiated)
            .unwrap();
        assert_eq!(*num, 16 * 1000);

        let builder = MultiThreadBuilder::new();
        let builder = builder.stack_size_by_qos(UserInteractive, 16 * 1000);
        let num = builder
            .common
            .stack_size_by_qos
            .get(&UserInteractive)
            .unwrap();
        assert_eq!(*num, 16 * 1000);

        let builder = MultiThreadBuilder::new();
        let builder = builder.stack_size_by_qos(Default, 16 * 1000 * 1000);
        let num = builder.common.stack_size_by_qos.get(&Default).unwrap();
        assert_eq!(*num, 16 * 1000 * 1000);

        let builder = MultiThreadBuilder::new();
        let builder = builder.stack_size_by_qos(UserInteractive, 16 * 1000 + 1);
        let num = builder
            .common
            .stack_size_by_qos
            .get(&UserInteractive)
            .unwrap();
        assert_eq!(*num, 16 * 1000 + 1);
    }
}

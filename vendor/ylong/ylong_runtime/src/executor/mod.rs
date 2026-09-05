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

//! Executor contains two parts:
//! - thread pool: how threads are started and how they run the tasks.
//! - schedule policy: how tasks are scheduled in the task queues.
pub(crate) mod block_on;
#[cfg(feature = "current_thread_runtime")]
pub(crate) mod current_thread;

use std::future::Future;
use std::mem::MaybeUninit;
use std::sync::Once;

use crate::builder::multi_thread_builder::GLOBAL_BUILDER;
use crate::builder::RuntimeBuilder;
#[cfg(feature = "current_thread_runtime")]
use crate::executor::current_thread::CurrentThreadSpawner;
use crate::task::{JoinHandle, Task, TaskBuilder};
mod driver_handle;
pub(crate) use driver_handle::Handle;

use crate::executor::worker::WorkerHandle;
cfg_ffrt! {
    use crate::ffrt::spawner::spawn;
}
cfg_not_ffrt! {
    mod parker;
    pub(crate) mod async_pool;
    pub(crate) mod blocking_pool;
    pub(crate) mod queue;
    mod sleeper;
    pub(crate) mod worker;
    pub(crate) mod driver;
    use crate::builder::{initialize_blocking_spawner, initialize_async_spawner};
    use crate::executor::async_pool::AsyncPoolSpawner;
    use crate::executor::blocking_pool::BlockPoolSpawner;
}

pub(crate) trait Schedule {
    fn schedule(&self, task: Task, lifo: bool);
}

pub(crate) struct PlaceholderScheduler;

impl Schedule for PlaceholderScheduler {
    fn schedule(&self, _task: Task, _lifo: bool) {
        panic!("no scheduler can be called");
    }
}

pub(crate) enum AsyncHandle {
    #[cfg(feature = "current_thread_runtime")]
    CurrentThread(CurrentThreadSpawner),
    #[cfg(not(feature = "ffrt"))]
    MultiThread(AsyncPoolSpawner),
    #[cfg(feature = "ffrt")]
    FfrtMultiThread,
}

/// Runtime struct.
///
/// # If `multi_instance_runtime` feature is turned on
/// There will be multiple runtime executors, initializing from user settings in
/// `RuntimeBuilder`.
///
/// # If `multi_instance_runtime` feature is turned off
/// There will be only *ONE* runtime executor singleton inside one process.
/// The async and blocking pools working when calling methods of this struct are
/// stored in the global static executor instance. Here, keep the empty struct
/// for compatibility and possibility for function extension in the future.
pub struct Runtime {
    pub(crate) async_spawner: AsyncHandle,
}

#[cfg(not(feature = "ffrt"))]
impl Runtime {
    pub(crate) fn get_handle(&self) -> std::sync::Arc<Handle> {
        match &self.async_spawner {
            #[cfg(feature = "current_thread_runtime")]
            AsyncHandle::CurrentThread(s) => s.handle.clone(),
            AsyncHandle::MultiThread(s) => s.exe_mng_info.handle.clone(),
        }
    }
}

pub(crate) fn global_default_async() -> &'static Runtime {
    static mut GLOBAL_DEFAULT_ASYNC: MaybeUninit<Runtime> = MaybeUninit::uninit();
    static ONCE: Once = Once::new();

    unsafe {
        ONCE.call_once(|| {
            let mut global_builder = GLOBAL_BUILDER.lock().unwrap();

            if global_builder.is_none() {
                *global_builder = Some(RuntimeBuilder::new_multi_thread());
            }

            #[cfg(not(feature = "ffrt"))]
            // we have just made sure the global builder is a some, so this unwrap_unchecked is safe
            let runtime = match initialize_async_spawner(global_builder.as_ref().unwrap_unchecked())
            {
                Ok(s) => Runtime {
                    async_spawner: AsyncHandle::MultiThread(s),
                },
                Err(e) => panic!("initialize runtime failed: {e:?}"),
            };
            #[cfg(feature = "ffrt")]
            let runtime = Runtime {
                async_spawner: AsyncHandle::FfrtMultiThread,
            };
            GLOBAL_DEFAULT_ASYNC = MaybeUninit::new(runtime);
        });
        &*GLOBAL_DEFAULT_ASYNC.as_ptr()
    }
}

#[cfg(not(feature = "ffrt"))]
pub(crate) fn global_default_blocking() -> &'static BlockPoolSpawner {
    static mut GLOBAL_DEFAULT_BLOCKING: MaybeUninit<BlockPoolSpawner> = MaybeUninit::uninit();
    static ONCE: Once = Once::new();

    unsafe {
        ONCE.call_once(|| {
            let mut global_builder = GLOBAL_BUILDER.lock().unwrap();

            if global_builder.is_none() {
                *global_builder = Some(RuntimeBuilder::new_multi_thread());
            }
            // we have just made sure the global builder is a some, so this unwrap_unchecked
            // is safe
            match initialize_blocking_spawner(&global_builder.as_ref().unwrap_unchecked().common) {
                Ok(bps) => GLOBAL_DEFAULT_BLOCKING = MaybeUninit::new(bps),
                Err(e) => panic!("initialize blocking pool failed: {e:?}"),
            }
        });
        &*GLOBAL_DEFAULT_BLOCKING.as_ptr()
    }
}

#[cfg(all(feature = "multi_instance_runtime", not(feature = "ffrt")))]
impl Runtime {
    /// Creates a new multi-thread runtime with default setting
    pub fn new() -> std::io::Result<Runtime> {
        RuntimeBuilder::new_multi_thread().build()
    }
}

impl Runtime {
    /// Spawns a future onto the runtime, returning a [`JoinHandle`] for it.
    ///
    /// The future will be later polled by the executor, which is usually
    /// implemented as a thread pool. The executor will run the future util
    /// finished.
    ///
    /// Awaits on the JoinHandle will return the result of the future, but users
    /// don't have to `.await` the `JoinHandle` in order to run the future,
    /// since the future will be executed in the background once it's
    /// spawned. Dropping the JoinHandle will throw away the returned value.
    ///
    /// # Examples
    ///
    /// ```no run
    /// use ylong_runtime::task::*;
    /// use ylong_runtime::builder::RuntimeBuilder;
    /// use ylong_runtime::executor::Runtime;
    ///
    /// async fn test_future(num: usize) -> usize {
    ///     num
    /// }
    ///
    /// let core_pool_size = 4;
    ///
    /// let runtime = RuntimeBuilder::new_multi_thread()
    ///     .worker_num(core_pool_size)
    ///     .build()
    ///     .unwrap();
    ///
    /// runtime.spawn(test_future(1));
    /// ```
    pub fn spawn<T, R>(&self, task: T) -> JoinHandle<R>
    where
        T: Future<Output = R> + Send + 'static,
        R: Send + 'static,
    {
        self.spawn_with_attr(task, &TaskBuilder::default())
    }

    #[inline]
    pub(crate) fn spawn_with_attr<T, R>(&self, task: T, builder: &TaskBuilder) -> JoinHandle<R>
    where
        T: Future<Output = R> + Send + 'static,
        R: Send + 'static,
    {
        match &self.async_spawner {
            #[cfg(feature = "current_thread_runtime")]
            AsyncHandle::CurrentThread(current_thread) => current_thread.spawn(builder, task),
            #[cfg(not(feature = "ffrt"))]
            AsyncHandle::MultiThread(async_spawner) => async_spawner.spawn(builder, task),
            #[cfg(feature = "ffrt")]
            AsyncHandle::FfrtMultiThread => spawn(task, builder),
        }
    }

    /// Spawns the provided function or closure onto the runtime.
    ///
    /// It's usually used for cpu-bounded computation that does not return
    /// pending and takes a relatively long time.
    ///
    /// # Examples
    ///
    /// ```no run
    /// use ylong_runtime::builder::RuntimeBuilder;
    ///
    /// use std::time;
    /// use std::thread::sleep;
    ///
    /// let runtime = RuntimeBuilder::new_multi_thread()
    ///     .build()
    ///     .unwrap();
    ///
    /// runtime.spawn_blocking(move || {
    ///     sleep(time::Duration::from_millis(1));
    ///     10
    /// });
    /// ```
    pub fn spawn_blocking<T, R>(&self, task: T) -> JoinHandle<R>
    where
        T: FnOnce() -> R + Send + 'static,
        R: Send + 'static,
    {
        crate::spawn::spawn_blocking(&TaskBuilder::new(), task)
    }

    /// Blocks the current thread and runs the given future (usually a
    /// JoinHandle) to completion, and gets its return value.
    ///
    /// Any code after the `block_on` will be executed once the future is done.
    ///
    /// # Panics
    /// 1. This function panics if it gets called in a runtime asynchronous
    ///    context. To be specific, this function cannot be called inside
    ///    `ylong_runtime::block_on` or `ylong_runtime::spawn`.
    /// 2. This function panics if the provided Future panics.
    ///
    /// # Examples
    ///
    /// ```no run
    /// use ylong_runtime::builder::RuntimeBuilder;
    ///
    /// let core_pool_size = 4;
    /// async fn test_future(num: usize) -> usize {
    ///     num
    /// }
    ///
    /// let runtime = RuntimeBuilder::new_multi_thread()
    ///     .worker_num(core_pool_size)
    ///     .build()
    ///     .unwrap();
    ///
    /// let handle = runtime.spawn(test_future(4));
    ///
    /// let result = runtime.block_on(handle);
    ///
    /// assert_eq!(result.unwrap(), 4);
    /// ```
    pub fn block_on<T, R>(&self, task: T) -> R
    where
        T: Future<Output = R>,
    {
        self.block_on_inner(task)
    }

    #[cfg(not(feature = "ffrt"))]
    fn block_on_inner<T, R>(&self, task: T) -> R
    where
        T: Future<Output = R>,
    {
        worker::CURRENT_HANDLE.with(|ctx| {
            if !ctx.get().is_null() {
                panic!(
                    "Cannot block_on an asynchronous function in a runtime context. \
                    This happens because a block_on call tries to block the current \
                    thread which is being used to drive asynchronous tasks."
                );
            }
        });

        // Registers handle to the current thread when block_on().
        // so that async_source can get the handle and register it.
        let cur_context = worker::WorkerHandle {
            _handle: self.get_handle(),
        };

        worker::CURRENT_HANDLE.with(|ctx| {
            ctx.set((&cur_context as *const WorkerHandle).cast::<()>());
        });

        let ret = match &self.async_spawner {
            #[cfg(feature = "current_thread_runtime")]
            AsyncHandle::CurrentThread(current_thread) => current_thread.block_on(task),
            AsyncHandle::MultiThread(_) => block_on::block_on(task),
        };

        // Sets the current thread variable to null,
        // otherwise the worker's CURRENT_WORKER can not be set under MultiThread.
        worker::CURRENT_HANDLE.with(|ctx| {
            ctx.set(std::ptr::null());
        });

        ret
    }

    #[cfg(feature = "ffrt")]
    fn block_on_inner<T, R>(&self, task: T) -> R
    where
        T: Future<Output = R>,
    {
        match &self.async_spawner {
            #[cfg(feature = "current_thread_runtime")]
            AsyncHandle::CurrentThread(current_thread) => current_thread.block_on(task),
            AsyncHandle::FfrtMultiThread => block_on::block_on(task),
        }
    }
}

cfg_metrics!(
    use crate::metrics::Metrics;
    impl Runtime {
        /// User can get some message from Runtime during running.
        ///
        /// # Example
        /// ```
        /// let runtime = ylong_runtime::builder::RuntimeBuilder::new_multi_thread().build().unwrap();
        /// let _metrics = runtime.metrics();
        /// ```
        pub fn metrics(&self) -> Metrics {
            Metrics::new(self)
        }
    }

    /// Gets metrics of the global Runtime.
    /// # Example
    /// ```
    /// use ylong_runtime::executor::get_global_runtime_metrics;
    ///
    /// let metrics = get_global_runtime_metrics();
    /// ```
    pub fn get_global_runtime_metrics() -> Metrics<'static> {
        Metrics::new(global_default_async())
    }
);

#[cfg(test)]
mod test {

    /// UT test cases for block_on inside a spawn
    ///
    /// # Brief
    /// 1. Call block_on inside a spawn
    /// 2. Check if the test panics
    #[should_panic]
    #[test]
    fn ut_block_on_panic_in_spawn() {
        let handle = crate::spawn(async move {
            let ret = crate::block_on(async move { 1 });
            assert_eq!(ret, 1);
        });
        crate::block_on(handle).unwrap();
    }

    /// UT test cases for new_timer_timeout
    ///
    /// # Brief
    /// 1. Call block inside another block_on
    /// 2. Check if the test panics
    #[should_panic]
    #[test]
    fn ut_block_on_panic_in_block_on() {
        crate::block_on(async move {
            let ret = crate::block_on(async move { 1 });
            assert_eq!(ret, 1);
        });
    }
}

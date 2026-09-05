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

use crate::executor::async_pool::AsyncPoolSpawner;
use crate::executor::{AsyncHandle, Runtime};

/// User can get some message from Runtime during running.
///
/// # Example
/// ```no_run
/// let runtime = ylong_runtime::builder::RuntimeBuilder::new_multi_thread()
///     .build()
///     .unwrap();
/// let metrics = runtime.metrics();
/// ```
pub struct Metrics<'a> {
    runtime: &'a Runtime,
}

/// List of workers state.
#[derive(Debug)]
pub struct WorkList {
    /// The set of index of the park workers
    pub park: Vec<usize>,
    /// The set of index of the active workers
    pub active: Vec<usize>,
}

impl Metrics<'_> {
    const ACTIVE_STATE: usize = 3;

    pub(crate) fn new(runtime: &Runtime) -> Metrics {
        Metrics { runtime }
    }

    /// Returns workers num
    ///
    /// # Example
    /// ```
    /// let runtime = ylong_runtime::builder::RuntimeBuilder::new_multi_thread()
    ///     .build()
    ///     .unwrap();
    /// let metrics = runtime.metrics();
    /// println!("Runtime's workers_num:{}", metrics.workers_num());
    /// ```
    pub fn workers_num(&self) -> usize {
        match &self.runtime.async_spawner {
            #[cfg(feature = "current_thread_runtime")]
            AsyncHandle::CurrentThread(_) => 1,
            AsyncHandle::MultiThread(spawner) => spawner.exe_mng_info.num_workers,
        }
    }

    /// Returns park workers num
    ///
    /// Runtime build by `new_current_thread()` will return None.
    ///
    /// # Example
    /// ```
    /// let runtime = ylong_runtime::builder::RuntimeBuilder::new_multi_thread()
    ///     .build()
    ///     .unwrap();
    /// let metrics = runtime.metrics();
    /// println!(
    ///     "Runtime's park_workers_num:{:?}",
    ///     metrics.park_workers_num()
    /// );
    /// ```
    pub fn park_workers_num(&self) -> Option<usize> {
        match &self.runtime.async_spawner {
            #[cfg(feature = "current_thread_runtime")]
            AsyncHandle::CurrentThread(_) => None,
            AsyncHandle::MultiThread(spawner) => {
                Some(Self::workers_state_statistic(spawner).park.len())
            }
        }
    }

    /// Returns active workers num
    ///
    /// Runtime build by `new_current_thread()` will return None.
    ///
    /// # Example
    /// ```
    /// let runtime = ylong_runtime::builder::RuntimeBuilder::new_multi_thread()
    ///     .build()
    ///     .unwrap();
    /// let metrics = runtime.metrics();
    /// println!(
    ///     "Runtime's active_workers_num:{:?}",
    ///     metrics.active_workers_num()
    /// );
    /// ```
    pub fn active_workers_num(&self) -> Option<usize> {
        match &self.runtime.async_spawner {
            #[cfg(feature = "current_thread_runtime")]
            AsyncHandle::CurrentThread(_) => None,
            AsyncHandle::MultiThread(spawner) => {
                Some(Self::workers_state_statistic(spawner).active.len())
            }
        }
    }

    /// Returns park workers index list
    ///
    /// Runtime build by `new_current_thread()` will return None.
    ///
    /// # Example
    /// ```
    /// let runtime = ylong_runtime::builder::RuntimeBuilder::new_multi_thread()
    ///     .build()
    ///     .unwrap();
    /// let metrics = runtime.metrics();
    /// println!(
    ///     "Runtime's park_workers_list:{:?}",
    ///     metrics.park_workers_list()
    /// );
    /// ```
    pub fn park_workers_list(&self) -> Option<Vec<usize>> {
        match &self.runtime.async_spawner {
            #[cfg(feature = "current_thread_runtime")]
            AsyncHandle::CurrentThread(_) => None,
            AsyncHandle::MultiThread(spawner) => Some(Self::workers_state_statistic(spawner).park),
        }
    }

    /// Returns active workers index list
    ///
    /// Runtime build by `new_current_thread()` will return None.
    ///
    /// # Example
    /// ```
    /// let runtime = ylong_runtime::builder::RuntimeBuilder::new_multi_thread()
    ///     .build()
    ///     .unwrap();
    /// let metrics = runtime.metrics();
    /// println!(
    ///     "Runtime's active_workers_list:{:?}",
    ///     metrics.active_workers_list()
    /// );
    /// ```
    pub fn active_workers_list(&self) -> Option<Vec<usize>> {
        match &self.runtime.async_spawner {
            #[cfg(feature = "current_thread_runtime")]
            AsyncHandle::CurrentThread(_) => None,
            AsyncHandle::MultiThread(spawner) => {
                Some(Self::workers_state_statistic(spawner).active)
            }
        }
    }

    /// Returns park/active workers index list
    ///
    /// Runtime build by `new_current_thread()` will return None.
    ///
    /// # Example
    /// ```
    /// let runtime = ylong_runtime::builder::RuntimeBuilder::new_multi_thread()
    ///     .build()
    ///     .unwrap();
    /// let metrics = runtime.metrics();
    /// println!(
    ///     "Runtime's overall_workers_list:{:?}",
    ///     metrics.overall_workers_list()
    /// );
    /// ```
    pub fn overall_workers_list(&self) -> Option<WorkList> {
        match &self.runtime.async_spawner {
            #[cfg(feature = "current_thread_runtime")]
            AsyncHandle::CurrentThread(_) => None,
            AsyncHandle::MultiThread(spawner) => Some(Self::workers_state_statistic(spawner)),
        }
    }

    fn workers_state_statistic(spawner: &AsyncPoolSpawner) -> WorkList {
        let mut park = vec![];
        let mut active = vec![];

        let parkers = spawner.exe_mng_info.get_handles().read().unwrap();
        for i in 0..parkers.len() {
            match parkers.get(i).unwrap().get_state() {
                Self::ACTIVE_STATE => active.push(i),
                _ => park.push(i),
            }
        }

        WorkList { park, active }
    }

    /// Returns global queue length
    ///
    /// # Example
    /// ```
    /// let runtime = ylong_runtime::builder::RuntimeBuilder::new_multi_thread()
    ///     .build()
    ///     .unwrap();
    /// let metrics = runtime.metrics();
    /// println!(
    ///     "Runtime's global_queue_length:{}",
    ///     metrics.global_queue_length()
    /// );
    /// ```
    pub fn global_queue_length(&self) -> usize {
        match &self.runtime.async_spawner {
            #[cfg(feature = "current_thread_runtime")]
            AsyncHandle::CurrentThread(spawner) => spawner.scheduler.inner.lock().unwrap().len(),
            AsyncHandle::MultiThread(spawner) => spawner.exe_mng_info.global.get_len(),
        }
    }

    /// Returns the total number of task which has entered global queue
    ///
    /// This value will only increment, not decrease.
    ///
    /// # Example
    /// ```
    /// let runtime = ylong_runtime::builder::RuntimeBuilder::new_multi_thread()
    ///     .build()
    ///     .unwrap();
    /// let metrics = runtime.metrics();
    /// println!(
    ///     "Runtime's global_queue_total_task_count:{}",
    ///     metrics.global_queue_total_task_count()
    /// );
    /// ```
    pub fn global_queue_total_task_count(&self) -> u64 {
        match &self.runtime.async_spawner {
            #[cfg(feature = "current_thread_runtime")]
            AsyncHandle::CurrentThread(spawner) => spawner
                .scheduler
                .count
                .load(std::sync::atomic::Ordering::Acquire),
            AsyncHandle::MultiThread(spawner) => spawner.exe_mng_info.global.get_count(),
        }
    }

    /// Returns the given worker thread length
    ///
    /// Runtime build by `new_current_thread()` will return None.
    ///
    /// # Example
    /// ```
    /// let runtime = ylong_runtime::builder::RuntimeBuilder::new_multi_thread()
    ///     .build()
    ///     .unwrap();
    /// let metrics = runtime.metrics();
    /// println!("Runtime's worker_task_len:{:?}", metrics.worker_task_len(0));
    /// ```
    pub fn worker_task_len(&self, index: usize) -> Option<usize> {
        match &self.runtime.async_spawner {
            #[cfg(feature = "current_thread_runtime")]
            AsyncHandle::CurrentThread(_) => None,
            AsyncHandle::MultiThread(spawner) => match spawner.get_worker(index) {
                Ok(worker) => {
                    let len = unsafe { worker.get_inner_ptr().run_queue.len() as usize };
                    Some(len)
                }
                Err(_) => panic!("out of index"),
            },
        }
    }

    /// Returns the total number of task which has entered the given worker
    /// thread
    ///
    /// This value will only increment, not decrease.
    /// Runtime build by `new_current_thread()` will return None.
    ///
    /// # Example
    /// ```
    /// let runtime = ylong_runtime::builder::RuntimeBuilder::new_multi_thread()
    ///     .build()
    ///     .unwrap();
    /// let metrics = runtime.metrics();
    /// println!(
    ///     "Runtime's worker_total_task_count:{:?}",
    ///     metrics.worker_total_task_count(0)
    /// );
    /// ```
    pub fn worker_total_task_count(&self, index: usize) -> Option<u64> {
        match &self.runtime.async_spawner {
            #[cfg(feature = "current_thread_runtime")]
            AsyncHandle::CurrentThread(_) => None,
            AsyncHandle::MultiThread(spawner) => match spawner.get_worker(index) {
                Ok(worker) => {
                    let len = unsafe { worker.get_inner_ptr().run_queue.count() };
                    Some(len)
                }
                Err(_) => panic!("out of index"),
            },
        }
    }

    /// Returns the number of task the given worker thread length has been
    /// polled.
    ///
    /// This value will only increment, not decrease.
    /// Runtime build by `new_current_thread()` will return None.
    ///
    /// # Example
    /// ```
    /// let runtime = ylong_runtime::builder::RuntimeBuilder::new_multi_thread()
    ///     .build()
    ///     .unwrap();
    /// let metrics = runtime.metrics();
    /// println!(
    ///     "Runtime's worker_poll_count:{:?}",
    ///     metrics.worker_poll_count(0)
    /// );
    /// ```
    pub fn worker_poll_count(&self, index: usize) -> Option<usize> {
        match &self.runtime.async_spawner {
            #[cfg(feature = "current_thread_runtime")]
            AsyncHandle::CurrentThread(_) => None,
            AsyncHandle::MultiThread(spawner) => match spawner.get_worker(index) {
                Ok(worker) => {
                    let len = unsafe { worker.get_inner_ptr().count as usize };
                    Some(len)
                }
                Err(_) => panic!("out of index"),
            },
        }
    }

    /// Returns the times of steals.
    ///
    /// This value will only increment, not decrease.
    /// Runtime build by `new_current_thread()` will return None.
    ///
    /// # Example
    /// ```
    /// let runtime = ylong_runtime::builder::RuntimeBuilder::new_multi_thread()
    ///     .build()
    ///     .unwrap();
    /// let metrics = runtime.metrics();
    /// println!("Runtime's steal_times:{:?}", metrics.steal_times());
    /// ```
    pub fn steal_times(&self) -> Option<u64> {
        match &self.runtime.async_spawner {
            #[cfg(feature = "current_thread_runtime")]
            AsyncHandle::CurrentThread(_) => None,
            AsyncHandle::MultiThread(spawner) => Some(spawner.exe_mng_info.get_steal_times()),
        }
    }

    /// Returns the number of times the given worker get tasks from the global
    /// queue.
    ///
    /// This value will only increment, not decrease.
    /// Runtime build by `new_current_thread()` will return None.
    ///
    /// # Example
    /// ```
    /// let runtime = ylong_runtime::builder::RuntimeBuilder::new_multi_thread()
    ///     .build()
    ///     .unwrap();
    /// let metrics = runtime.metrics();
    /// println!(
    ///     "Runtime's worker_get_task_from_global_count:{:?}",
    ///     metrics.worker_get_task_from_global_count(0)
    /// );
    /// ```
    pub fn worker_get_task_from_global_count(&self, index: usize) -> Option<u64> {
        match &self.runtime.async_spawner {
            #[cfg(feature = "current_thread_runtime")]
            AsyncHandle::CurrentThread(_) => None,
            AsyncHandle::MultiThread(spawner) => match spawner.get_worker(index) {
                Ok(worker) => {
                    let len = unsafe { worker.get_inner_ptr().run_queue.task_from_global_count() };
                    Some(len)
                }
                Err(_) => panic!("out of index"),
            },
        }
    }

    /// Returns the number of times the given worker push a task on the global
    /// queue.
    ///
    /// This value will only increment, not decrease.
    /// Runtime build by `new_current_thread()` will return None.
    ///
    /// # Example
    /// ```
    /// let runtime = ylong_runtime::builder::RuntimeBuilder::new_multi_thread()
    ///     .build()
    ///     .unwrap();
    /// let metrics = runtime.metrics();
    /// println!(
    ///     "Runtime's worker_push_task_to_global_count:{:?}",
    ///     metrics.worker_push_task_to_global_count(0)
    /// );
    /// ```
    pub fn worker_push_task_to_global_count(&self, index: usize) -> Option<u64> {
        match &self.runtime.async_spawner {
            #[cfg(feature = "current_thread_runtime")]
            AsyncHandle::CurrentThread(_) => None,
            AsyncHandle::MultiThread(spawner) => match spawner.get_worker(index) {
                Ok(worker) => {
                    let len = unsafe { worker.get_inner_ptr().run_queue.task_to_global_count() };
                    Some(len)
                }
                Err(_) => panic!("out of index"),
            },
        }
    }

    /// Returns the number of IO events which has been registered in Driver.
    ///
    /// This value will only increment, not decrease.
    ///
    /// # Example
    /// ```
    /// let runtime = ylong_runtime::builder::RuntimeBuilder::new_multi_thread()
    ///     .build()
    ///     .unwrap();
    /// let metrics = runtime.metrics();
    /// println!(
    ///     "Runtime's fd_registered_count:{}",
    ///     metrics.fd_registered_count()
    /// );
    /// ```
    #[cfg(feature = "net")]
    pub fn fd_registered_count(&self) -> u64 {
        self.runtime.get_handle().get_registered_count()
    }

    /// Returns the number of IO events which has been readied in Driver.
    ///
    /// This value will only increment, not decrease.
    ///
    /// # Example
    /// ```
    /// let runtime = ylong_runtime::builder::RuntimeBuilder::new_multi_thread()
    ///     .build()
    ///     .unwrap();
    /// let metrics = runtime.metrics();
    /// println!(
    ///     "Runtime's io_driver_ready_count:{}",
    ///     metrics.io_driver_ready_count()
    /// );
    /// ```
    #[cfg(feature = "net")]
    pub fn io_driver_ready_count(&self) -> u64 {
        self.runtime.get_handle().get_ready_count()
    }
}

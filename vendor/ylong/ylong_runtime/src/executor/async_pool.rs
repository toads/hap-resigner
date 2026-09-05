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

use std::cell::RefCell;
use std::future::Future;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::{Acquire, SeqCst};
use std::sync::{Arc, Condvar, Mutex, RwLock};
use std::time::Duration;
use std::{cmp, io, thread};

use super::driver::{Driver, Handle};
use super::parker::Parker;
use super::queue::{GlobalQueue, LocalQueue, LOCAL_QUEUE_CAP};
use super::sleeper::Sleeper;
use super::worker::{get_current_ctx, run_worker, Worker};
use super::{worker, Schedule};
use crate::builder::multi_thread_builder::MultiThreadBuilder;
use crate::builder::CallbackHook;
use crate::executor::worker::WorkerContext;
use crate::fastrand::fast_random;
use crate::task::{JoinHandle, Task, TaskBuilder, VirtualTableType};
#[cfg(not(target_os = "macos"))]
use crate::util::core_affinity::set_current_affinity;
use crate::util::num_cpus::get_cpu_num;

const ASYNC_THREAD_QUIT_WAIT_TIME: Duration = Duration::from_secs(3);
pub(crate) const GLOBAL_POLL_INTERVAL: u8 = 61;

pub(crate) struct MultiThreadScheduler {
    /// Async pool shutdown state
    is_cancel: AtomicBool,
    /// Number of total workers
    pub(crate) num_workers: usize,
    /// Join Handles for all threads in the executor
    handles: RwLock<Vec<Parker>>,
    /// Used for idle and wakeup logic.
    pub(crate) sleeper: Sleeper,
    /// The global queue of the executor
    pub(crate) global: GlobalQueue,
    /// A set of all the local queues in the executor
    locals: Vec<LocalQueue>,
    pub(crate) handle: Arc<Handle>,
    #[cfg(feature = "metrics")]
    steal_times: std::sync::atomic::AtomicU64,
}

impl Schedule for MultiThreadScheduler {
    #[inline]
    fn schedule(&self, task: Task, lifo: bool) {
        if self.enqueue(task, lifo) {
            self.wake_up_rand_one(false);
        }
    }
}

impl MultiThreadScheduler {
    pub(crate) fn new(thread_num: usize, handle: Arc<Handle>) -> Self {
        let mut locals = Vec::new();
        for _ in 0..thread_num {
            locals.push(LocalQueue::new());
        }

        Self {
            is_cancel: AtomicBool::new(false),
            num_workers: thread_num,
            handles: RwLock::new(Vec::new()),
            sleeper: Sleeper::new(thread_num),
            global: GlobalQueue::new(),
            locals,
            handle,
            #[cfg(feature = "metrics")]
            steal_times: std::sync::atomic::AtomicU64::new(0),
        }
    }

    pub(crate) fn is_cancel(&self) -> bool {
        self.is_cancel.load(Acquire)
    }

    pub(crate) fn set_cancel(&self) {
        self.is_cancel.store(true, SeqCst);
    }

    pub(crate) fn cancel(&self) {
        self.set_cancel();
        self.wake_up_all();
    }

    fn wake_up_all(&self) {
        let join_handle = self.handles.read().unwrap();
        for item in join_handle.iter() {
            item.unpark(self.handle.clone());
        }
    }

    #[inline]
    pub(crate) fn is_parked(&self, worker_index: &usize) -> bool {
        self.sleeper.is_parked(worker_index)
    }

    pub(crate) fn is_waked_by_last_search(&self, idx: usize) -> bool {
        let mut search_list = self.sleeper.wake_by_search.lock().unwrap();
        let is_waked_by_last_search = search_list[idx];
        search_list[idx] = false;
        if is_waked_by_last_search {
            self.sleeper.inc_searching_num();
            return true;
        }
        false
    }

    pub(crate) fn wake_up_rand_one_if_last_search(&self) {
        if self.sleeper.dec_searching_num() {
            self.wake_up_rand_one(true);
        }
    }

    pub(crate) fn wake_up_rand_one(&self, last_search: bool) {
        if let Some(index) = self.sleeper.pop_worker(last_search) {
            // index is bounded by total worker num
            self.handles
                .read()
                .unwrap()
                .get(index)
                .unwrap()
                .unpark(self.handle.clone());
        }
    }

    pub(crate) fn turn_to_sleep(&self, worker_inner: &mut worker::Inner, worker_index: usize) {
        let is_last_search = if worker_inner.is_searching {
            worker_inner.is_searching = false;
            self.sleeper.dec_searching_num()
        } else {
            false
        };
        let is_last_active = self.sleeper.push_worker(worker_index);

        if (is_last_search || is_last_active) && !self.has_no_work() {
            self.wake_up_rand_one(true);
        }
    }

    #[inline]
    pub(crate) fn turn_from_sleep(&self, worker_index: &usize) {
        self.sleeper.pop_worker_by_id(worker_index);
    }

    pub(crate) fn create_local_queue(&self, index: usize) -> LocalQueue {
        // this index is bounded by total worker num
        let local_run_queue = self.locals.get(index).unwrap();
        LocalQueue {
            inner: local_run_queue.inner.clone(),
        }
    }

    pub(crate) fn has_no_work(&self) -> bool {
        // check if local queues are empty
        for index in 0..self.num_workers {
            // this index is bounded by total worker num
            let item = self.locals.get(index).unwrap();
            if !item.is_empty() {
                return false;
            }
        }
        // then check is global queue empty
        self.global.is_empty()
    }

    // The returned value indicates whether or not to wake up another worker
    fn enqueue_under_ctx(&self, mut task: Task, worker_ctx: &WorkerContext, lifo: bool) -> bool {
        // if the current context is another runtime, push it to the global queue
        if !std::ptr::eq(&self.global, &worker_ctx.worker.scheduler.global) {
            self.global.push_back(task);
            return true;
        }

        if lifo {
            let mut lifo_slot = worker_ctx.worker.lifo.borrow_mut();
            let prev_task = lifo_slot.take();
            if let Some(prev) = prev_task {
                // there is some task in lifo slot, therefore we put the prev task
                // into run queue, and put the current task into the lifo slot
                *lifo_slot = Some(task);
                task = prev;
            } else {
                // there is no task in lifo slot, return immediately
                *lifo_slot = Some(task);
                return false;
            }
        }

        // this index is bounded by total worker num
        let local_run_queue = self.locals.get(worker_ctx.worker.index).unwrap();
        local_run_queue.push_back(task, &self.global);
        true
    }

    // The returned value indicates whether or not to wake up another worker
    // We need to wake another worker under these circumstances:
    // 1. The task has been inserted into the global queue
    // 2. The lifo slot is taken, we push the old task into the local queue
    pub(crate) fn enqueue(&self, task: Task, lifo: bool) -> bool {
        let cur_worker = get_current_ctx();

        // currently we are inside a runtime's context
        if let Some(worker_ctx) = cur_worker {
            return self.enqueue_under_ctx(task, worker_ctx, lifo);
        }

        // If the local queue of the current worker is full, push the task into the
        // global queue
        self.global.push_back(task);
        true
    }

    // gets task from the global queue or the thread's own local queue
    fn get_task_from_queues(&self, worker_inner: &mut worker::Inner) -> Option<Task> {
        let count = worker_inner.count;
        let local_run_queue = &worker_inner.run_queue;

        // For every 61 times of execution, dequeue a task from the global queue first.
        // Otherwise, dequeue a task from the local queue. However, if the local queue
        // has no task, dequeue a task from the global queue instead.
        if count % GLOBAL_POLL_INTERVAL as u32 == 0 {
            let mut limit = local_run_queue.remaining() as usize;
            // If the local queue is empty, multiple tasks are stolen from the global queue
            // to the local queue. If the local queue has tasks, only dequeue one task from
            // the global queue and run it.
            if limit != LOCAL_QUEUE_CAP {
                limit = 0;
            }
            let task = self
                .global
                .pop_batch(self.num_workers, local_run_queue, limit);
            match task {
                Some(task) => Some(task),
                None => local_run_queue.pop_front(),
            }
        } else {
            let local_task = local_run_queue.pop_front();
            match local_task {
                Some(task) => Some(task),
                None => {
                    let limit = local_run_queue.remaining() as usize;
                    self.global
                        .pop_batch(self.num_workers, local_run_queue, limit)
                }
            }
        }
    }

    fn get_task_from_searching(&self, worker_inner: &mut worker::Inner) -> Option<Task> {
        const STEAL_TIME: usize = 3;

        // There is no task in the local queue or the global queue, so we try to steal
        // tasks from another worker's local queue.
        // The number of stealing workers should be less than half of the total worker
        // number.
        // Only increases the searching number only when the worker is not searching
        if !worker_inner.is_searching && !self.sleeper.try_inc_searching_num() {
            return None;
        }

        worker_inner.is_searching = true;

        let local_run_queue = &worker_inner.run_queue;
        for i in 0..STEAL_TIME {
            if let Some(task) = self.steal(local_run_queue) {
                return Some(task);
            }
            if i < STEAL_TIME - 1 {
                thread::sleep(Duration::from_micros(1));
            }
        }

        None
    }

    pub(crate) fn dequeue(
        &self,
        worker_inner: &mut worker::Inner,
        worker_ctx: &WorkerContext,
    ) -> Option<Task> {
        // dequeues from the global queue or the thread's own local queue
        if let Some(task) = self.get_task_from_queues(worker_inner) {
            return Some(task);
        }

        if let Ok(mut driver) = worker_inner.parker.get_driver().try_lock() {
            driver.run_once();
        }
        worker_ctx.wake_yield();
        if !worker_inner.run_queue.is_empty() {
            return None;
        }

        self.get_task_from_searching(worker_inner)
    }

    fn steal(&self, destination: &LocalQueue) -> Option<Task> {
        let num = self.locals.len();
        let start = (fast_random() >> 56) as usize;

        for i in 0..num {
            let i = (start + i) % num;
            // skip the current worker's local queue
            // this index is bounded by total worker num
            let target = self.locals.get(i).unwrap();

            if std::ptr::eq(target, destination) {
                continue;
            }

            if let Some(task) = target.steal_into(destination) {
                #[cfg(feature = "metrics")]
                self.steal_times
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                return Some(task);
            }
        }

        // if there is no task to steal, we check global queue for tasks
        self.global.pop_batch(
            self.num_workers,
            destination,
            destination.remaining() as usize,
        )
    }

    cfg_metrics!(
        pub(crate) fn get_handles(&self) -> &RwLock<Vec<Parker>> {
            &self.handles
        }

        pub(crate) fn get_steal_times(&self) -> u64 {
            self.steal_times.load(Acquire)
        }
    );
}

#[derive(Clone)]
pub(crate) struct AsyncPoolSpawner {
    pub(crate) inner: Arc<Inner>,

    pub(crate) exe_mng_info: Arc<MultiThreadScheduler>,
}

impl Drop for AsyncPoolSpawner {
    fn drop(&mut self) {
        self.release()
    }
}

pub(crate) struct Inner {
    /// Number of total threads
    pub(crate) total: usize,
    /// Core-affinity setting of the threads
    #[cfg_attr(target_os = "macos", allow(unused))]
    is_affinity: bool,
    /// Handle for shutting down the pool
    shutdown_handle: Arc<(Mutex<usize>, Condvar)>,
    /// A callback func to be called after thread starts
    after_start: Option<CallbackHook>,
    /// A callback func to be called before thread stops
    before_stop: Option<CallbackHook>,
    /// Name of the worker threads
    worker_name: Option<String>,
    /// Stack size of each thread
    stack_size: Option<usize>,
    /// Workers
    #[cfg(feature = "metrics")]
    workers: Mutex<Vec<Arc<Worker>>>,
}

fn get_cpu_core() -> usize {
    cmp::max(1, get_cpu_num() as usize)
}

fn async_thread_proc(inner: Arc<Inner>, worker: Arc<Worker>, handle: Arc<Handle>) {
    if let Some(f) = inner.after_start.clone() {
        f();
    }

    run_worker(worker, handle);
    let (lock, cvar) = &*(inner.shutdown_handle.clone());
    let mut finished = lock.lock().unwrap();
    *finished += 1;

    // the last thread wakes up the main thread
    if *finished >= inner.total {
        cvar.notify_one();
    }

    if let Some(f) = inner.before_stop.clone() {
        f();
    }
}

impl AsyncPoolSpawner {
    pub(crate) fn new(builder: &MultiThreadBuilder) -> io::Result<Self> {
        let (handle, driver) = Driver::initialize();

        let thread_num = builder.core_thread_size.unwrap_or_else(get_cpu_core);
        let spawner = AsyncPoolSpawner {
            inner: Arc::new(Inner {
                total: thread_num,
                is_affinity: builder.common.is_affinity,
                shutdown_handle: Arc::new((Mutex::new(0), Condvar::new())),
                after_start: builder.common.after_start.clone(),
                before_stop: builder.common.before_stop.clone(),
                worker_name: builder.common.worker_name.clone(),
                stack_size: builder.common.stack_size,
                #[cfg(feature = "metrics")]
                workers: Mutex::new(Vec::with_capacity(thread_num)),
            }),
            exe_mng_info: Arc::new(MultiThreadScheduler::new(thread_num, handle)),
        };
        spawner.create_async_thread_pool(driver)?;
        Ok(spawner)
    }

    fn create_async_thread_pool(&self, driver: Arc<Mutex<Driver>>) -> io::Result<()> {
        let mut workers = vec![];
        for index in 0..self.inner.total {
            let local_queue = self.exe_mng_info.create_local_queue(index);
            let local_run_queue =
                Box::new(worker::Inner::new(local_queue, Parker::new(driver.clone())));
            workers.push(Arc::new(Worker {
                index,
                scheduler: self.exe_mng_info.clone(),
                inner: RefCell::new(local_run_queue),
                lifo: RefCell::new(None),
                yielded: RefCell::new(Vec::new()),
            }))
        }

        for (worker_id, worker) in workers.drain(..).enumerate() {
            let work_arc_handle = self.exe_mng_info.handle.clone();
            #[cfg(feature = "metrics")]
            self.inner.workers.lock().unwrap().push(worker.clone());
            // set up thread attributes
            let mut builder = thread::Builder::new();

            if let Some(worker_name) = self.inner.worker_name.clone() {
                builder = builder.name(format!("async-{worker_id}-{worker_name}"));
            } else {
                builder = builder.name(format!("async-{worker_id}"));
            }

            if let Some(stack_size) = self.inner.stack_size {
                builder = builder.stack_size(stack_size);
            }

            let parker = worker.inner.borrow().parker.clone();
            self.exe_mng_info.handles.write().unwrap().push(parker);

            let inner = self.inner.clone();

            #[cfg(not(target_os = "macos"))]
            if self.inner.is_affinity {
                builder.spawn(move || {
                    let cpu_core_num = get_cpu_core();
                    let cpu_id = worker_id % cpu_core_num;
                    let _ = set_current_affinity(cpu_id);
                    async_thread_proc(inner, worker, work_arc_handle);
                })?;
            } else {
                builder.spawn(move || {
                    async_thread_proc(inner, worker, work_arc_handle);
                })?;
            }

            #[cfg(target_os = "macos")]
            builder.spawn(move || {
                async_thread_proc(inner, worker, work_arc_handle);
            })?;
        }
        Ok(())
    }

    pub(crate) fn spawn<T>(&self, builder: &TaskBuilder, task: T) -> JoinHandle<T::Output>
    where
        T: Future + Send + 'static,
        T::Output: Send + 'static,
    {
        let exe_scheduler = Arc::downgrade(&self.exe_mng_info);
        let (task, join_handle) =
            Task::create_task(builder, exe_scheduler, task, VirtualTableType::Ylong);

        self.exe_mng_info.schedule(task, false);
        join_handle
    }

    /// # Safety
    /// Users need to guarantee that the future will remember lifetime and thus
    /// compiler will capture lifetime issues, or the future will complete
    /// when its context remains valid. If not, currently
    /// runtime initialization will cause memory error.
    ///
    /// ## Memory issue example
    /// No matter using which type (current / multi thread) of runtime, the
    /// following code can compile. When the variable `slice` gets released
    /// when the function ends, any handles returned from this function rely
    /// on a dangled pointer.
    ///
    /// ```no run
    ///  fn err_example(runtime: &Runtime) -> JoinHandle<()> {
    ///     let builder = TaskBuilder::default();
    ///     let mut slice = [1, 2, 3, 4, 5];
    ///     let borrow = &mut slice;
    ///     match &runtime.async_spawner {
    ///         AsyncHandle::CurrentThread(pool) => {
    ///             pool.spawn_with_ref(
    ///                 &builder,
    ///                 async { borrow.iter_mut().for_each(|x| *x *= 2) }
    ///             )
    ///        }
    ///        AsyncHandle::MultiThread(pool) => {
    ///             pool.spawn_with_ref(
    ///                 &builder,
    ///                 async { borrow.iter_mut().for_each(|x| *x *= 2) }
    ///             )
    ///        }
    ///     }
    /// }
    ///
    /// let runtime = Runtime::new().unwrap();
    /// let handle = spawn_blocking(
    ///     move || block_on(err_example(&runtime)).unwrap()
    /// );
    /// ```
    pub(crate) unsafe fn spawn_with_ref<T>(
        &self,
        builder: &TaskBuilder,
        task: T,
    ) -> JoinHandle<T::Output>
    where
        T: Future + Send,
        T::Output: Send,
    {
        let exe_scheduler = Arc::downgrade(&self.exe_mng_info);
        let raw_task = Task::create_raw_task(builder, exe_scheduler, task, VirtualTableType::Ylong);
        let handle = JoinHandle::new(raw_task);
        let task = Task(raw_task);
        self.exe_mng_info.schedule(task, false);
        handle
    }

    /// Waits 3 seconds for threads to finish before releasing the async pool.
    /// If threads could not finish before releasing, there could be possible
    /// memory leak.
    fn release_wait(&self) -> Result<(), ()> {
        self.exe_mng_info.cancel();
        let pair = self.inner.shutdown_handle.clone();
        let total = self.inner.total;
        let (lock, cvar) = &*pair;
        let finished = lock.lock().unwrap();
        let res = cvar
            .wait_timeout_while(finished, ASYNC_THREAD_QUIT_WAIT_TIME, |&mut finished| {
                finished < total
            })
            .unwrap();
        // if time limit has been reached, the unfinished threads would not get released
        if res.1.timed_out() {
            Err(())
        } else {
            Ok(())
        }
    }

    pub(crate) fn release(&self) {
        if let Ok(()) = self.release_wait() {
            let mut join_handle = self.exe_mng_info.handles.write().unwrap();
            #[allow(clippy::mem_replace_with_default)]
            let mut worker_handles = std::mem::replace(join_handle.as_mut(), vec![]);
            drop(join_handle);
            for parker in worker_handles.drain(..) {
                parker.release();
            }
        }
    }

    #[cfg(feature = "metrics")]
    pub(crate) fn get_worker(&self, index: usize) -> Result<Arc<Worker>, ()> {
        let vec = self.inner.workers.lock().unwrap();
        for worker in vec.iter() {
            if worker.index == index {
                return Ok(worker.clone());
            }
        }
        Err(())
    }
}

#[cfg(test)]
pub(crate) mod test {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::Ordering::{Acquire, Release};
    use std::sync::mpsc::channel;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll};
    use std::thread;

    use crate::builder::RuntimeBuilder;
    use crate::executor::async_pool::{get_cpu_core, AsyncPoolSpawner, MultiThreadScheduler};
    use crate::executor::driver::Driver;
    use crate::executor::parker::Parker;
    use crate::executor::queue::LocalQueue;
    use crate::executor::{worker, Schedule};
    use crate::task::{JoinHandle, Task, TaskBuilder, VirtualTableType};

    pub struct TestFuture {
        value: usize,
        total: usize,
    }

    pub fn create_new() -> TestFuture {
        TestFuture {
            value: 0,
            total: 1000,
        }
    }

    impl Future for TestFuture {
        type Output = usize;
        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            if self.total > self.value {
                self.get_mut().value += 1;
                cx.waker().wake_by_ref();
                Poll::Pending
            } else {
                Poll::Ready(self.total)
            }
        }
    }

    async fn test_future() -> usize {
        create_new().await
    }

    /// UT test cases for ExecutorMngInfo::new()
    ///
    /// # Brief
    /// 1. Creates a ExecutorMsgInfo with thread number 1
    /// 2. Creates a ExecutorMsgInfo with thread number 2
    #[test]
    fn ut_executor_mng_info_new_001() {
        let (arc_handle, _) = Driver::initialize();
        let executor_mng_info = MultiThreadScheduler::new(1, arc_handle.clone());
        assert!(!executor_mng_info.is_cancel.load(Acquire));
        assert_eq!(executor_mng_info.handles.read().unwrap().capacity(), 0);

        let executor_mng_info = MultiThreadScheduler::new(64, arc_handle);
        assert!(!executor_mng_info.is_cancel.load(Acquire));
        assert_eq!(executor_mng_info.handles.read().unwrap().capacity(), 0);
    }

    /// UT test cases for ExecutorMngInfo::create_local_queues()
    ///
    /// # Brief
    /// 1. index set to 0, check the return value
    /// 2. index set to ExecutorMngInfo.inner.total, check the return value
    #[test]
    fn ut_executor_mng_info_create_local_queues() {
        let (arc_handle, _) = Driver::initialize();
        let executor_mng_info = MultiThreadScheduler::new(1, arc_handle.clone());
        let local_run_queue_info = executor_mng_info.create_local_queue(0);
        assert!(local_run_queue_info.is_empty());

        let executor_mng_info = MultiThreadScheduler::new(64, arc_handle);
        let local_run_queue_info = executor_mng_info.create_local_queue(63);
        assert!(local_run_queue_info.is_empty());
    }

    pub(crate) fn create_task<T, S>(
        builder: &TaskBuilder,
        scheduler: std::sync::Weak<S>,
        task: T,
        virtual_table_type: VirtualTableType,
    ) -> (Task, JoinHandle<T::Output>)
    where
        T: Future + Send + 'static,
        T::Output: Send + 'static,
        S: Schedule,
    {
        let (task, handle) = Task::create_task(builder, scheduler, task, virtual_table_type);
        (task, handle)
    }

    /// UT test cases for ExecutorMngInfo::enqueue()
    ///
    /// # Brief
    /// 1. index set to 0, check the return value
    /// 2. index set to ExecutorMngInfo.inner.total, check the return value
    #[test]
    fn ut_executor_mng_info_enqueue() {
        let (arc_handle, _) = Driver::initialize();
        let executor_mng_info = MultiThreadScheduler::new(1, arc_handle.clone());

        let builder = TaskBuilder::new();
        let exe_scheduler = Arc::downgrade(&Arc::new(MultiThreadScheduler::new(1, arc_handle)));
        let (task, _) = create_task(
            &builder,
            exe_scheduler,
            test_future(),
            VirtualTableType::Ylong,
        );

        executor_mng_info.enqueue(task, true);
        assert!(!executor_mng_info.has_no_work());
    }

    /// UT test cases for ExecutorMngInfo::is_cancel()
    ///
    /// # Brief
    /// 1. The is_cancel value is set to true to check the return value
    /// 2. The is_cancel value is set to false to check the return value
    #[test]
    fn ut_executor_mng_info_is_cancel() {
        let (arc_handle, _) = Driver::initialize();
        let executor_mng_info = MultiThreadScheduler::new(1, arc_handle);
        executor_mng_info.is_cancel.store(false, Release);
        assert!(!executor_mng_info.is_cancel());
        executor_mng_info.is_cancel.store(true, Release);
        assert!(executor_mng_info.is_cancel());
    }

    /// UT test cases for ExecutorMngInfo::set_cancel()
    ///
    /// # Brief
    /// 1. Check if the is_cancel parameter becomes true after set_cancel
    #[test]
    fn ut_executor_mng_info_set_cancel() {
        let (arc_handle, _) = Driver::initialize();
        let executor_mng_info = MultiThreadScheduler::new(1, arc_handle);
        assert!(!executor_mng_info.is_cancel.load(Acquire));
        executor_mng_info.set_cancel();
        assert!(executor_mng_info.is_cancel.load(Acquire));
    }

    /// UT test cases for ExecutorMngInfo::cancel()
    ///
    /// # Brief
    /// 1. Check if the is_cancel parameter becomes true after set_cancel
    #[test]
    fn ut_executor_mng_info_cancel() {
        let (arc_handle, arc_driver) = Driver::initialize();
        let executor_mng_info = MultiThreadScheduler::new(1, arc_handle);

        let flag = Arc::new(Mutex::new(0));
        let (tx, rx) = channel();

        let (flag_clone, tx) = (flag.clone(), tx);

        let mut parker = Parker::new(arc_driver);
        let parker_cpy = parker.clone();
        let _ = thread::spawn(move || {
            parker.park();
            *flag_clone.lock().unwrap() = 1;
            tx.send(()).unwrap()
        });
        executor_mng_info.handles.write().unwrap().push(parker_cpy);

        executor_mng_info.cancel();
        rx.recv().unwrap();
        assert_eq!(*flag.lock().unwrap(), 1);
    }

    /// UT test cases for ExecutorMngInfo::wake_up_all()
    ///
    /// # Brief
    /// 1. Constructs an environment to check if all threads are woken up and
    ///    executed via thread hooks.
    #[test]
    fn ut_executor_mng_info_wake_up_all() {
        let (arc_handle, arc_driver) = Driver::initialize();
        let executor_mng_info = MultiThreadScheduler::new(1, arc_handle);

        let flag = Arc::new(Mutex::new(0));
        let (tx, rx) = channel();

        let (flag_clone, tx) = (flag.clone(), tx);

        let mut parker = Parker::new(arc_driver);
        let parker_cpy = parker.clone();

        let _ = thread::spawn(move || {
            parker.park();
            *flag_clone.lock().unwrap() = 1;
            tx.send(()).unwrap()
        });

        executor_mng_info.handles.write().unwrap().push(parker_cpy);

        executor_mng_info.wake_up_all();
        rx.recv().unwrap();
        assert_eq!(*flag.lock().unwrap(), 1);
    }

    /// UT test cases for ExecutorMngInfo::wake_up_rand_one()
    ///
    /// # Brief
    /// 1. Constructs an environment to check if a thread is woken up and
    ///    executed by a thread hook.
    #[test]
    fn ut_executor_mng_info_wake_up_rand_one() {
        let (arc_handle, arc_driver) = Driver::initialize();
        let mut parker = Parker::new(arc_driver);
        let executor_mng_info = MultiThreadScheduler::new(1, arc_handle);
        let local_queue = LocalQueue {
            inner: executor_mng_info.locals[0].inner.clone(),
        };
        let mut worker_inner = worker::Inner::new(local_queue, parker.clone());
        worker_inner.is_searching = true;
        executor_mng_info.sleeper.inc_searching_num();
        executor_mng_info.turn_to_sleep(&mut worker_inner, 0);

        let flag = Arc::new(Mutex::new(0));
        let (tx, rx) = channel();

        let (flag_clone, tx) = (flag.clone(), tx);
        let parker_cpy = parker.clone();

        let _ = thread::spawn(move || {
            parker.park();
            *flag_clone.lock().unwrap() = 1;
            tx.send(()).unwrap()
        });

        executor_mng_info.handles.write().unwrap().push(parker_cpy);
        executor_mng_info.wake_up_rand_one(false);
        rx.recv().unwrap();
        assert_eq!(*flag.lock().unwrap(), 1);
        assert_eq!(executor_mng_info.sleeper.pop_worker(false), None);
    }

    /// UT test cases for ExecutorMngInfo::wake_up_if_one_task_left()
    ///
    /// # Brief
    /// 1. Constructs the environment, checks if there are still tasks, and if
    ///    so, wakes up a thread to continue working.
    #[test]
    fn ut_executor_mng_info_wake_up_if_one_task_left() {
        let (arc_handle, arc_driver) = Driver::initialize();
        let mut parker = Parker::new(arc_driver);
        let executor_mng_info = MultiThreadScheduler::new(1, arc_handle.clone());

        let local_queue = LocalQueue {
            inner: executor_mng_info.locals[0].inner.clone(),
        };
        let mut worker_inner = worker::Inner::new(local_queue, parker.clone());
        executor_mng_info.turn_to_sleep(&mut worker_inner, 0);

        let flag = Arc::new(Mutex::new(0));
        let (tx, rx) = channel();

        let (flag_clone, tx) = (flag.clone(), tx);
        let parker_cpy = parker.clone();

        let _ = thread::spawn(move || {
            parker.park();
            *flag_clone.lock().unwrap() = 1;
            tx.send(()).unwrap()
        });

        executor_mng_info.handles.write().unwrap().push(parker_cpy);

        let builder = TaskBuilder::new();
        let exe_scheduler = Arc::downgrade(&Arc::new(MultiThreadScheduler::new(1, arc_handle)));
        let (task, _) = create_task(
            &builder,
            exe_scheduler,
            test_future(),
            VirtualTableType::Ylong,
        );

        executor_mng_info.enqueue(task, true);

        if !executor_mng_info.has_no_work() {
            executor_mng_info.wake_up_rand_one(false);
        }

        rx.recv().unwrap();
        assert_eq!(*flag.lock().unwrap(), 1);
        assert_eq!(executor_mng_info.sleeper.pop_worker(false), None);
    }

    /// UT test cases for ExecutorMngInfo::from_woken_to_sleep()
    ///
    /// # Brief
    ///  1. Construct the environment and set the state of the specified thread
    ///     to park state. If the last thread is in park state, check whether
    ///     there is a task, and if so, wake up this thread.
    #[test]
    fn ut_executor_mng_info_from_woken_to_sleep() {
        let (arc_handle, arc_driver) = Driver::initialize();
        let executor_mng_info = MultiThreadScheduler::new(1, arc_handle.clone());

        let flag = Arc::new(Mutex::new(0));
        let (tx, rx) = channel();
        let (flag_clone, tx) = (flag.clone(), tx);

        let mut parker = Parker::new(arc_driver);

        let local_queue = LocalQueue {
            inner: executor_mng_info.locals[0].inner.clone(),
        };
        let mut worker_inner = worker::Inner::new(local_queue, parker.clone());
        worker_inner.is_searching = true;
        executor_mng_info.sleeper.inc_searching_num();

        let parker_cpy = parker.clone();

        let _ = thread::spawn(move || {
            parker.park();
            *flag_clone.lock().unwrap() = 1;
            tx.send(()).unwrap()
        });

        executor_mng_info.handles.write().unwrap().push(parker_cpy);

        let builder = TaskBuilder::new();
        let exe_scheduler = Arc::downgrade(&Arc::new(MultiThreadScheduler::new(1, arc_handle)));
        let (task, _) = create_task(
            &builder,
            exe_scheduler,
            test_future(),
            VirtualTableType::Ylong,
        );

        executor_mng_info.enqueue(task, true);
        executor_mng_info.turn_to_sleep(&mut worker_inner, 0);
        rx.recv().unwrap();
        assert_eq!(*flag.lock().unwrap(), 1);
        assert_eq!(executor_mng_info.sleeper.pop_worker(false), None);
    }

    /// UT test cases for AsyncPoolSpawner::new()
    ///
    /// # Brief
    /// 1. Verify the parameters of the initialization completion
    #[test]
    fn ut_async_pool_spawner_new() {
        let thread_pool_builder = RuntimeBuilder::new_multi_thread();
        let async_pool_spawner = AsyncPoolSpawner::new(&thread_pool_builder).unwrap();
        assert_eq!(
            async_pool_spawner.inner.total,
            thread_pool_builder
                .core_thread_size
                .unwrap_or_else(get_cpu_core)
        );
        assert_eq!(
            async_pool_spawner.inner.worker_name,
            thread_pool_builder.common.worker_name
        );
        assert_eq!(
            async_pool_spawner.inner.stack_size,
            thread_pool_builder.common.stack_size
        );
        assert!(!async_pool_spawner.exe_mng_info.is_cancel.load(Acquire));
    }

    /// UT test cases for `create_async_thread_pool`.
    ///
    /// # Brief
    /// 1. Create an async_pool_spawner with `is_affinity` setting to false
    /// 2. Call create_async_thread_pool()
    /// 3. This UT should not panic
    #[test]
    fn ut_async_pool_spawner_create_async_thread_pool_001() {
        let thread_pool_builder = RuntimeBuilder::new_multi_thread();
        let _ = AsyncPoolSpawner::new(&thread_pool_builder.is_affinity(false)).unwrap();
    }

    /// UT test cases for `UnboundedSender`.
    ///
    /// # Brief
    /// 1. Create an async_pool_spawner with `is_affinity` setting to true
    /// 2. Call create_async_thread_pool()
    /// 3. This UT should not panic
    #[test]
    fn ut_async_pool_spawner_create_async_thread_pool_002() {
        let thread_pool_builder = RuntimeBuilder::new_multi_thread();
        let _ = AsyncPoolSpawner::new(&thread_pool_builder.is_affinity(true)).unwrap();
    }
}

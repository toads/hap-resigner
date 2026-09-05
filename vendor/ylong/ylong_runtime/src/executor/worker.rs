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

use std::cell::{Cell, RefCell};
use std::ptr;
use std::sync::Arc;
use std::task::Waker;

/// worker struct info and method
use crate::executor::async_pool::MultiThreadScheduler;
use crate::executor::driver::Handle;
use crate::executor::parker::Parker;
use crate::executor::queue::LocalQueue;
use crate::task::Task;

thread_local! {
    pub(crate) static CURRENT_WORKER: Cell<* const ()> = Cell::new(ptr::null());
    pub(crate) static CURRENT_HANDLE: Cell<* const ()> = Cell::new(ptr::null());
}

pub(crate) struct WorkerContext {
    pub(crate) worker: Arc<Worker>,
}

impl WorkerContext {
    #[inline]
    fn run(&mut self) {
        let worker_ref = &self.worker;
        worker_ref.run(self);
    }

    pub(crate) fn wake_yield(&self) -> bool {
        let mut yielded = self.worker.yielded.borrow_mut();
        if yielded.is_empty() {
            return false;
        }
        for waker in yielded.drain(..) {
            waker.wake();
        }
        true
    }

    #[inline]
    fn release(&mut self) {
        self.worker.release();
    }
}

pub(crate) struct WorkerHandle {
    pub(crate) _handle: Arc<Handle>,
}

/// Gets the handle of the current thread
#[cfg(all(not(feature = "ffrt"), any(feature = "net", feature = "time")))]
#[inline]
pub(crate) fn get_current_handle() -> Option<&'static WorkerHandle> {
    CURRENT_HANDLE.with(|ctx| {
        let val = ctx.get();
        if val.is_null() {
            None
        } else {
            Some(unsafe { &*(val.cast::<WorkerHandle>()) })
        }
    })
}

/// Gets the worker context of the current thread
#[inline]
pub(crate) fn get_current_ctx() -> Option<&'static WorkerContext> {
    CURRENT_WORKER.with(|ctx| {
        let val = ctx.get();
        if val.is_null() {
            None
        } else {
            Some(unsafe { &*(val.cast::<WorkerContext>()) })
        }
    })
}

/// Runs the worker thread
pub(crate) fn run_worker(worker: Arc<Worker>, handle: Arc<Handle>) {
    let mut cur_context = WorkerContext { worker };

    let cur_handle = WorkerHandle { _handle: handle };

    struct Reset(*const (), *const ());

    impl Drop for Reset {
        fn drop(&mut self) {
            CURRENT_WORKER.with(|ctx| ctx.set(self.0));
            CURRENT_HANDLE.with(|handle| handle.set(self.1));
        }
    }
    // store the worker to tls
    let _guard = CURRENT_WORKER.with(|cur| {
        let prev_ctx = cur.get();
        cur.set((&cur_context as *const WorkerContext).cast::<()>());

        let handle = CURRENT_HANDLE.with(|handle| {
            let prev_handle = handle.get();
            handle.set((&cur_handle as *const WorkerHandle).cast::<()>());
            prev_handle
        });

        Reset(prev_ctx, handle)
    });

    cur_context.run();
    cur_context.release();
    drop(cur_handle);
}

pub(crate) struct Worker {
    pub(crate) index: usize,
    pub(crate) scheduler: Arc<MultiThreadScheduler>,
    pub(crate) inner: RefCell<Box<Inner>>,
    pub(crate) lifo: RefCell<Option<Task>>,
    pub(crate) yielded: RefCell<Vec<Waker>>,
}

unsafe impl Send for Worker {}
unsafe impl Sync for Worker {}

impl Worker {
    fn run(&self, worker_ctx: &WorkerContext) {
        let mut inner = self.inner.borrow_mut();
        let inner = inner.as_mut();

        while !inner.is_cancel() {
            inner.increment_count();
            inner.periodic_check(self);

            if let Some(task) = self.get_task(inner, worker_ctx) {
                if inner.is_searching {
                    inner.is_searching = false;
                    self.scheduler.wake_up_rand_one_if_last_search();
                }
                task.run();
                continue;
            }

            // if there is no task, park the worker
            self.park_timeout(inner, worker_ctx);
            self.check_cancel(inner);

            if !inner.is_searching && self.scheduler.is_waked_by_last_search(self.index) {
                inner.is_searching = true;
            }
        }
    }

    fn get_task(&self, inner: &mut Inner, worker_ctx: &WorkerContext) -> Option<Task> {
        // schedule lifo task first
        let mut lifo_slot = worker_ctx.worker.lifo.borrow_mut();
        if let Some(task) = lifo_slot.take() {
            return Some(task);
        }

        self.scheduler.dequeue(inner, worker_ctx)
    }

    #[inline]
    fn check_cancel(&self, inner: &mut Inner) {
        inner.check_cancel(self)
    }

    fn has_work(&self, inner: &mut Inner, worker_ctx: &WorkerContext) -> bool {
        worker_ctx.worker.lifo.borrow().is_some() || !inner.run_queue.is_empty()
    }

    fn park_timeout(&self, inner: &mut Inner, worker_ctx: &WorkerContext) {
        // still has works to do, go back to work
        if self.has_work(inner, worker_ctx) {
            return;
        }
        self.scheduler.turn_to_sleep(inner, self.index);
        inner.is_searching = false;

        while !inner.is_cancel {
            inner.parker.park();

            if self.has_work(inner, worker_ctx) {
                self.scheduler.turn_from_sleep(&self.index);
                break;
            }

            if self.scheduler.is_parked(&self.index) {
                self.check_cancel(inner);
                continue;
            }
            break;
        }
    }

    /// Gets Worker's Inner with ptr.
    ///
    /// # Safety
    /// We can't get Inner with `RefCell::borrow()`, because the worker will
    /// always hold the borrow_mut until drop. So we can only get Inner by ptr.
    /// This method can only be used to obtain values
    #[cfg(feature = "metrics")]
    pub(crate) unsafe fn get_inner_ptr(&self) -> &Inner {
        let ptr = self.inner.as_ptr();
        &*ptr
    }

    #[inline]
    fn release(&self) {
        // wait for tasks in queue to finish
        while !self.scheduler.has_no_work() {}
    }
}

pub(crate) struct Inner {
    /// A counter to define whether schedule global queue or local queue
    pub(crate) count: u32,
    /// Whether the workers are canceled
    is_cancel: bool,
    /// local queue
    pub(crate) run_queue: LocalQueue,
    pub(crate) parker: Parker,
    pub(crate) is_searching: bool,
}

impl Inner {
    pub(crate) fn new(run_queues: LocalQueue, parker: Parker) -> Self {
        Inner {
            count: 0,
            is_cancel: false,
            run_queue: run_queues,
            parker,
            is_searching: false,
        }
    }
}

const GLOBAL_PERIODIC_INTERVAL: u8 = 61;

impl Inner {
    #[inline]
    fn increment_count(&mut self) {
        self.count = self.count.wrapping_add(1);
    }

    // checks if the worker is canceled
    #[inline]
    fn check_cancel(&mut self, worker: &Worker) {
        if !self.is_cancel {
            self.is_cancel = worker.scheduler.is_cancel();
        }
    }

    #[inline]
    fn periodic_check(&mut self, worker: &Worker) {
        if self.count & GLOBAL_PERIODIC_INTERVAL as u32 == 0 {
            self.check_cancel(worker);
            if let Ok(mut driver) = self.parker.get_driver().try_lock() {
                driver.run_once();
            }
        }
    }

    #[inline]
    fn is_cancel(&self) -> bool {
        self.is_cancel
    }
}

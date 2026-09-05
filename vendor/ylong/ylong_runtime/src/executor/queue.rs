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

use std::cell::UnsafeCell;
use std::mem::MaybeUninit;
#[cfg(feature = "metrics")]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering::{AcqRel, Acquire, Release, SeqCst};
use std::sync::atomic::{AtomicU16, AtomicU32, AtomicUsize};
use std::sync::{Arc, Mutex};
use std::{cmp, ptr};

/// Schedule strategy implementation, includes FIFO LIFO priority and
/// work-stealing work-stealing strategy include stealing half of every worker
/// or the largest amount of worker
use crate::task::{Header, Task};
use crate::util::linked_list::LinkedList;

unsafe fn non_atomic_load(data: &AtomicU16) -> u16 {
    ptr::read((data as *const AtomicU16).cast::<u16>())
}

/// Capacity of the local queue
pub(crate) const LOCAL_QUEUE_CAP: usize = 256;
const MASK: u16 = LOCAL_QUEUE_CAP as u16 - 1;

/// Local queue of the worker
pub(crate) struct LocalQueue {
    pub(crate) inner: Arc<InnerBuffer>,
}

unsafe impl Send for LocalQueue {}
unsafe impl Sync for LocalQueue {}

unsafe impl Send for InnerBuffer {}
unsafe impl Sync for InnerBuffer {}

impl LocalQueue {
    pub(crate) fn new() -> Self {
        LocalQueue {
            inner: Arc::new(InnerBuffer::new(LOCAL_QUEUE_CAP as u16)),
        }
    }

    fn is_half_full(&self, rear: u16) -> bool {
        let (steal_pos, _) = unwrap(self.inner.front.load(Acquire));
        if rear.wrapping_sub(steal_pos) > LOCAL_QUEUE_CAP as u16 / 2 {
            return true;
        }
        false
    }
}

#[inline]
fn unwrap(num: u32) -> (u16, u16) {
    let head_pos = num & u16::MAX as u32;
    let steal_pos = num >> 16;
    (steal_pos as u16, head_pos as u16)
}

#[inline]
fn wrap(steal_pos: u16, head_pos: u16) -> u32 {
    (head_pos as u32) | ((steal_pos as u32) << 16)
}

impl LocalQueue {
    #[inline]
    pub(crate) fn pop_front(&self) -> Option<Task> {
        self.inner.pop_front()
    }

    #[inline]
    pub(crate) fn push_back(&self, task: Task, global: &GlobalQueue) {
        self.inner.push_back(task, global);
    }

    #[inline]
    pub(crate) fn steal_into(&self, dst: &LocalQueue) -> Option<Task> {
        self.inner.steal_into(dst)
    }

    #[inline]
    pub(crate) fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    #[inline]
    pub(crate) fn remaining(&self) -> u16 {
        self.inner.remaining()
    }
}

#[cfg(feature = "metrics")]
impl LocalQueue {
    #[inline]
    pub(crate) fn len(&self) -> u16 {
        self.inner.len()
    }

    #[inline]
    pub(crate) fn count(&self) -> u64 {
        self.inner.count()
    }

    #[inline]
    pub(crate) fn task_from_global_count(&self) -> u64 {
        self.inner.task_from_global_count()
    }

    #[inline]
    pub(crate) fn task_to_global_count(&self) -> u64 {
        self.inner.task_to_global_count()
    }
}

pub(crate) struct InnerBuffer {
    /// Front stores the position of both head and steal
    front: AtomicU32,
    rear: AtomicU16,
    cap: u16,
    buffer: Box<[UnsafeCell<MaybeUninit<Task>>]>,
    #[cfg(feature = "metrics")]
    metrics: InnerBufferMetrics,
}

/// Metrics of InnerBuffer
#[cfg(feature = "metrics")]
struct InnerBufferMetrics {
    /// The total number of task which has entered this LocalQueue
    count: AtomicU64,
    /// The total number of task which has entered this LocalQueue from
    /// GlobalQueue
    task_from_global_count: AtomicU64,
    /// The total number of task which has entered GlobalQueue from this
    /// LocalQueue
    task_to_global_count: AtomicU64,
}

#[cfg(feature = "metrics")]
impl InnerBuffer {
    /// Return queue's len.
    fn len(&self) -> u16 {
        let rear = self.rear.load(Acquire);
        let (_, head) = unwrap(self.front.load(Acquire));
        rear.wrapping_sub(head)
    }

    /// Returns the total number of task which has entered this LocalQueue
    fn count(&self) -> u64 {
        self.metrics.count.load(Acquire)
    }

    /// Returns the total number of task which has entered this LocalQueue from
    /// GlobalQueue
    fn task_from_global_count(&self) -> u64 {
        self.metrics.task_from_global_count.load(Acquire)
    }

    /// Returns the total number of task which has entered GlobalQueue from this
    /// LocalQueue
    fn task_to_global_count(&self) -> u64 {
        self.metrics.task_to_global_count.load(Acquire)
    }
}

impl InnerBuffer {
    fn new(cap: u16) -> Self {
        let mut buffer = Vec::with_capacity(cap as usize);

        for _ in 0..cap {
            buffer.push(UnsafeCell::new(MaybeUninit::uninit()));
        }
        InnerBuffer {
            front: AtomicU32::new(0),
            rear: AtomicU16::new(0),
            cap,
            buffer: buffer.into(),
            #[cfg(feature = "metrics")]
            metrics: InnerBufferMetrics {
                count: AtomicU64::new(0),
                task_from_global_count: AtomicU64::new(0),
                task_to_global_count: AtomicU64::new(0),
            },
        }
    }

    /// Checks whether the queue is empty
    fn is_empty(&self) -> bool {
        let (_, head) = unwrap(self.front.load(Acquire));
        let rear = self.rear.load(Acquire);
        head == rear
    }

    pub(crate) fn pop_front(&self) -> Option<Task> {
        let mut head = self.front.load(Acquire);

        let pos = loop {
            let (steal_pos, real_pos) = unwrap(head);

            // it's a spmc queue, so the queue could read its own tail non-atomically
            let tail_pos = unsafe { non_atomic_load(&self.rear) };

            // return none if the queue is empty
            if real_pos == tail_pos {
                return None;
            }

            let next_real = real_pos.wrapping_add(1);
            let next = if steal_pos == real_pos {
                wrap(next_real, next_real)
            } else {
                wrap(steal_pos, next_real)
            };

            let res = self.front.compare_exchange(head, next, AcqRel, Acquire);
            match res {
                Ok(_) => break real_pos,
                Err(actual) => head = actual,
            }
        };

        let task = self.buffer[(pos & MASK) as usize].get();

        Some(unsafe { ptr::read(task).assume_init() })
    }

    pub(crate) fn remaining(&self) -> u16 {
        let front = self.front.load(Acquire);

        let (steal_pos, _real_pos) = unwrap(front);
        // it's a spmc queue, so the queue could read its own tail non-atomically
        let rear = unsafe { non_atomic_load(&self.rear) };

        self.cap - (rear.wrapping_sub(steal_pos))
    }

    fn sync_steal_pos(&self, mut prev: u32) {
        loop {
            let (_front_steal, front_real) = unwrap(prev);
            let next = wrap(front_real, front_real);
            let res = self.front.compare_exchange(prev, next, AcqRel, Acquire);

            if let Err(actual) = res {
                let (actual_steal_pos, actual_real_pos) = unwrap(actual);
                assert_ne!(
                    actual_steal_pos, actual_real_pos,
                    "steal pos: {}, real_pos: {}, they should not be the same",
                    actual_steal_pos, actual_real_pos
                );
                prev = actual;
            } else {
                return;
            }
        }
    }

    pub(crate) fn push_back(&self, mut task: Task, global: &GlobalQueue) {
        loop {
            let front = self.front.load(Acquire);

            let (steal_pos, _) = unwrap(front);
            // it's a spmc queue, so the queue could read its own tail non-atomically
            let rear = unsafe { non_atomic_load(&self.rear) };

            // if the local queue is full, push the task into the global queue
            if rear.wrapping_sub(steal_pos) < self.cap {
                let idx = (rear & MASK) as usize;
                let ptr = self.buffer[idx].get();
                unsafe {
                    ptr::write((*ptr).as_mut_ptr(), task);
                }
                self.rear.store(rear.wrapping_add(1), SeqCst);
                #[cfg(feature = "metrics")]
                self.metrics.count.fetch_add(1, AcqRel);
                return;
            } else {
                match self.push_overflowed(task, global, steal_pos) {
                    Ok(_) => return,
                    Err(ret) => task = ret,
                }
            }
        }
    }

    #[allow(unused_assignments)]
    pub(crate) fn push_overflowed(
        &self,
        task: Task,
        global: &GlobalQueue,
        front: u16,
    ) -> Result<(), Task> {
        // get the number of tasks the worker has stolen
        let count = LOCAL_QUEUE_CAP / 2;
        let prev = wrap(front, front);
        let next = wrap(front, front.wrapping_add(count as u16));

        match self.front.compare_exchange(prev, next, Release, Acquire) {
            Ok(_) => {}
            Err(_) => return Err(task),
        }

        let (mut src_front_steal, _src_front_real) = unwrap(prev);

        let mut tmp_buf = Vec::with_capacity(count);
        for _ in 0..count {
            tmp_buf.push(UnsafeCell::new(MaybeUninit::uninit()));
        }

        for dst_ptr in tmp_buf.iter().take(count) {
            let src_idx = (src_front_steal & MASK) as usize;
            let task_ptr = self.buffer[src_idx].get();
            let task = unsafe { ptr::read(task_ptr).assume_init() };
            unsafe {
                ptr::write((*dst_ptr.get()).as_mut_ptr(), task);
            }
            src_front_steal = src_front_steal.wrapping_add(1);
        }

        self.sync_steal_pos(next);

        #[cfg(feature = "metrics")]
        self.metrics
            .task_to_global_count
            .fetch_add(tmp_buf.len() as u64 + 1, AcqRel);

        global.push_batch(tmp_buf, task);

        Ok(())
    }

    pub(crate) fn steal_into(&self, dst: &LocalQueue) -> Option<Task> {
        // it's a spmc queue, so the queue could read its own tail non-atomically
        let mut dst_rear = unsafe { non_atomic_load(&dst.inner.rear) };
        if dst.is_half_full(dst_rear) {
            return None;
        }

        let mut src_next_front;
        let mut src_prev_front = self.front.load(Acquire);

        // get the number of tasks the worker has stolen
        let mut count = loop {
            let (src_front_steal, src_front_real) = unwrap(src_prev_front);

            // if these two values are not equal, it means another worker has stolen from
            // this queue, therefore abort this steal.
            if src_front_steal != src_front_real {
                return None;
            };

            let src_rear = self.rear.load(Acquire);

            // steal half of the tasks from the queue
            let mut n = src_rear.wrapping_sub(src_front_real);
            n = n - n / 2;
            if n == 0 {
                return None;
            }

            let src_steal_to = src_front_real.wrapping_add(n);
            src_next_front = wrap(src_front_steal, src_steal_to);

            let res = self
                .front
                .compare_exchange(src_prev_front, src_next_front, AcqRel, Acquire);
            match res {
                Ok(_) => break n,
                Err(actual) => src_prev_front = actual,
            }
        };

        // transfer the tasks
        let (mut src_front_steal, _src_front_real) = unwrap(src_next_front);
        count -= 1;
        for _ in 0..count {
            let src_idx = (src_front_steal & MASK) as usize;
            let des_idx = (dst_rear & MASK) as usize;

            let task_ptr = self.buffer[src_idx].get();

            let task = unsafe { ptr::read(task_ptr).assume_init() };
            let ptr = dst.inner.buffer[des_idx].get();

            unsafe {
                ptr::write((*ptr).as_mut_ptr(), task);
            }
            src_front_steal = src_front_steal.wrapping_add(1);
            dst_rear = dst_rear.wrapping_add(1);
        }

        let src_idx = (src_front_steal & MASK) as usize;

        let task_ptr = self.buffer[src_idx].get();
        let task = unsafe { ptr::read(task_ptr).assume_init() };
        if count != 0 {
            dst.inner.rear.store(dst_rear, SeqCst);
        }

        self.sync_steal_pos(src_next_front);

        Some(task)
    }
}

impl Drop for InnerBuffer {
    fn drop(&mut self) {
        let mut head = self.pop_front();
        while let Some(task) = head {
            task.shutdown();
            head = self.pop_front();
        }
    }
}

pub(crate) struct GlobalQueue {
    /// Current number of tasks
    len: AtomicUsize,
    /// The total number of tasks which has entered global queue.
    #[cfg(feature = "metrics")]
    count: AtomicU64,
    globals: Mutex<LinkedList<Header>>,
}

impl Drop for GlobalQueue {
    fn drop(&mut self) {
        while !self.is_empty() {
            // we just check the queue is not empty
            let task = self.pop_front().unwrap();
            task.shutdown();
        }
    }
}

impl GlobalQueue {
    pub(crate) fn new() -> Self {
        GlobalQueue {
            len: AtomicUsize::new(0_usize),
            #[cfg(feature = "metrics")]
            count: AtomicU64::new(0),
            globals: Mutex::new(LinkedList::new()),
        }
    }
    pub(super) fn is_empty(&self) -> bool {
        self.len.load(Acquire) == 0
    }

    pub(super) fn push_batch(&self, tasks: Vec<UnsafeCell<MaybeUninit<Task>>>, task: Task) {
        let mut list = self.globals.lock().unwrap();
        let len = tasks.len() + 1;
        for task_ptr in tasks {
            let task = unsafe { ptr::read(task_ptr.get()).assume_init() };
            list.push_front(task.into_header());
        }
        list.push_front(task.into_header());
        self.len.fetch_add(len, AcqRel);
        #[cfg(feature = "metrics")]
        self.count.fetch_add(len as u64, AcqRel);
    }

    pub(super) fn pop_batch(
        &self,
        worker_num: usize,
        queue: &LocalQueue,
        limit: usize,
    ) -> Option<Task> {
        let len = self.len.load(Acquire);
        let num = cmp::min(len / worker_num, limit);

        let inner_buf = &queue.inner;
        // it's a spmc queue, so the queue could read its own tail non-atomically
        let rear = unsafe { non_atomic_load(&inner_buf.rear) };
        let mut curr = rear;

        let mut list = self.globals.lock().unwrap();
        let first_task = unsafe { Task::from_raw(list.pop_back()?) };

        let mut count = 1;

        for _ in 1..num {
            if let Some(task) = list.pop_back() {
                let idx = (curr & MASK) as usize;
                let ptr = inner_buf.buffer[idx].get();
                unsafe {
                    ptr::write((*ptr).as_mut_ptr(), Task::from_raw(task));
                }
                curr = curr.wrapping_add(1);
                count += 1;
            } else {
                break;
            }
        }
        drop(list);
        self.len.fetch_sub(count, AcqRel);
        inner_buf.rear.store(curr, Release);

        #[cfg(feature = "metrics")]
        inner_buf
            .metrics
            .task_from_global_count
            .fetch_add(1, AcqRel);

        Some(first_task)
    }

    pub(super) fn pop_front(&self) -> Option<Task> {
        if self.is_empty() {
            return None;
        }
        let mut list = self.globals.lock().unwrap();
        let task = list
            .pop_back()
            .map(|header| unsafe { Task::from_raw(header) });
        if task.is_some() {
            self.len.fetch_sub(1, AcqRel);
        }
        drop(list);
        task
    }

    pub(super) fn push_back(&self, task: Task) {
        let mut list = self.globals.lock().unwrap();
        let header = task.into_header();
        list.push_front(header);
        self.len.fetch_add(1, AcqRel);
        drop(list);
        #[cfg(feature = "metrics")]
        self.count.fetch_add(1, AcqRel);
    }

    #[cfg(feature = "metrics")]
    pub(crate) fn get_len(&self) -> usize {
        self.len.load(Acquire)
    }

    #[cfg(feature = "metrics")]
    pub(crate) fn get_count(&self) -> u64 {
        self.count.load(Acquire)
    }
}

#[cfg(test)]
pub(crate) mod test {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::atomic::Ordering::Acquire;
    use std::sync::Arc;
    use std::task::{Context, Poll};
    use std::thread::park;

    use crate::executor::async_pool::test::create_task;
    use crate::executor::async_pool::MultiThreadScheduler;
    use crate::executor::driver::Driver;
    use crate::executor::queue::{GlobalQueue, InnerBuffer, LocalQueue, LOCAL_QUEUE_CAP};
    use crate::task::{TaskBuilder, VirtualTableType};

    #[cfg(any(not(feature = "metrics"), feature = "ffrt"))]
    impl InnerBuffer {
        fn len(&self) -> u16 {
            let front = self.front.load(Acquire);
            let (_, real_pos) = crate::executor::queue::unwrap(front);

            let rear = self.rear.load(Acquire);
            rear.wrapping_sub(real_pos)
        }
    }

    #[cfg(any(not(feature = "metrics"), feature = "ffrt"))]
    impl LocalQueue {
        pub fn len(&self) -> u16 {
            self.inner.len()
        }
    }

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

    impl LocalQueue {
        fn pop_front_and_release(&self) {
            let task = self.pop_front();
            if let Some(task) = task {
                task.shutdown();
            }
        }

        fn steal_into_and_release(&self, other: &LocalQueue) {
            let task = self.steal_into(other);
            if let Some(task) = task {
                task.shutdown();
            }
        }
    }

    /// UT test cases for InnerBuffer::new()
    ///
    /// # Brief
    /// 1. Checking the parameters after initialization is completed
    #[test]
    fn ut_inner_buffer_new() {
        let inner_buffer = InnerBuffer::new(LOCAL_QUEUE_CAP as u16);
        assert_eq!(inner_buffer.cap, LOCAL_QUEUE_CAP as u16);
        assert_eq!(inner_buffer.buffer.len(), LOCAL_QUEUE_CAP);
    }

    /// InnerBuffer::is_empty() UT test cases
    ///
    /// # Brief
    /// case execution
    /// 1. Checking the parameters after initialization iscompleted
    /// 2. After entering a task into the queue space, determine again whether
    ///    it is empty or not, and it should be non-empty property value should
    ///    be related to the entry after the initialization is completed
    #[test]
    fn ut_inner_buffer_is_empty() {
        let inner_buffer = InnerBuffer::new(LOCAL_QUEUE_CAP as u16);
        assert!(inner_buffer.is_empty());

        let builder = TaskBuilder::new();

        let (arc_handle, _) = Driver::initialize();

        let exe_scheduler = Arc::downgrade(&Arc::new(MultiThreadScheduler::new(1, arc_handle)));
        let (task, _) = create_task(
            &builder,
            exe_scheduler,
            test_future(),
            VirtualTableType::Ylong,
        );
        let global_queue = GlobalQueue::new();
        let inner_buffer = InnerBuffer::new(LOCAL_QUEUE_CAP as u16);
        inner_buffer.push_back(task, &global_queue);
        assert!(!inner_buffer.is_empty());
    }

    /// InnerBuffer::len() UT test cases
    ///
    /// # Brief
    /// case execution
    /// 1. Checking the parameters after initialization is completed
    /// 2. Insert tasks up to their capacity into the local queue, checking the
    ///    local queue length
    /// 3. Insert tasks into the local queue that exceed its capacity, checking
    ///    the local queue length as well as the global queue length value, no
    ///    exception branch, and the property value should be related to the
    ///    entry after the initialization is completed
    #[test]
    fn ut_inner_buffer_len() {
        let inner_buffer = InnerBuffer::new(LOCAL_QUEUE_CAP as u16);
        assert_eq!(inner_buffer.len(), 0);

        let inner_buffer = InnerBuffer::new(LOCAL_QUEUE_CAP as u16);
        let global_queue = GlobalQueue::new();
        let builder = TaskBuilder::new();

        let (arc_handle, _) = Driver::initialize();

        let exe_scheduler =
            Arc::downgrade(&Arc::new(MultiThreadScheduler::new(1, arc_handle.clone())));
        let (task, _) = create_task(
            &builder,
            exe_scheduler,
            test_future(),
            VirtualTableType::Ylong,
        );
        inner_buffer.push_back(task, &global_queue);
        assert_eq!(inner_buffer.len(), 1);

        let inner_buffer = InnerBuffer::new(LOCAL_QUEUE_CAP as u16);
        let global_queue = GlobalQueue::new();
        for _ in 0..LOCAL_QUEUE_CAP + 1 {
            let exe_scheduler =
                Arc::downgrade(&Arc::new(MultiThreadScheduler::new(1, arc_handle.clone())));
            let (task, _) = create_task(
                &builder,
                exe_scheduler,
                test_future(),
                VirtualTableType::Ylong,
            );
            inner_buffer.push_back(task, &global_queue);
        }
        assert_eq!(
            inner_buffer.len() as usize,
            LOCAL_QUEUE_CAP - LOCAL_QUEUE_CAP / 2
        );
        assert_eq!(global_queue.len.load(Acquire), 1 + LOCAL_QUEUE_CAP / 2);
    }

    /// InnerBuffer::push_back() UT test cases
    ///
    /// # Brief
    /// case execution
    /// 1. Insert tasks up to capacity into the local queue, verifying that they
    ///    are functionally correct
    /// 2. Insert tasks that exceed the capacity into the local queue and verify
    ///    that they are functionally correct there is an exception branch,
    ///    after the initialization is completed the property value should be
    ///    related to the entry
    #[test]
    fn ut_inner_buffer_push_back() {
        // 1. Insert tasks up to capacity into the local queue, verifying that they are
        // functionally correct
        let local_queue = LocalQueue::new();
        let global_queue = GlobalQueue::new();

        let (arc_handle, _) = Driver::initialize();

        let builder = TaskBuilder::new();
        for _ in 0..LOCAL_QUEUE_CAP / 2 {
            let exe_scheduler =
                Arc::downgrade(&Arc::new(MultiThreadScheduler::new(2, arc_handle.clone())));
            let (task, _) = create_task(
                &builder,
                exe_scheduler,
                test_future(),
                VirtualTableType::Ylong,
            );
            local_queue.push_back(task, &global_queue);
        }

        for _ in 0..LOCAL_QUEUE_CAP / 2 {
            let exe_scheduler =
                Arc::downgrade(&Arc::new(MultiThreadScheduler::new(2, arc_handle.clone())));
            let (task, _) = create_task(
                &builder,
                exe_scheduler,
                test_future(),
                VirtualTableType::Ylong,
            );
            local_queue.push_back(task, &global_queue);
        }

        assert_eq!(local_queue.len(), 256);

        // 2. Insert tasks that exceed the capacity into the local queue and verify that
        // they are functionally correct
        let local_queue = LocalQueue::new();
        let global_queue = GlobalQueue::new();

        let (arc_handle, _) = Driver::initialize();

        for _ in 0..LOCAL_QUEUE_CAP / 2 + 1 {
            let exe_scheduler =
                Arc::downgrade(&Arc::new(MultiThreadScheduler::new(2, arc_handle.clone())));
            let (task, _) = create_task(
                &builder,
                exe_scheduler,
                test_future(),
                VirtualTableType::Ylong,
            );
            local_queue.push_back(task, &global_queue);
        }

        for _ in 0..LOCAL_QUEUE_CAP / 2 {
            let exe_scheduler =
                Arc::downgrade(&Arc::new(MultiThreadScheduler::new(2, arc_handle.clone())));
            let (task, _) = create_task(
                &builder,
                exe_scheduler,
                test_future(),
                VirtualTableType::Ylong,
            );
            local_queue.push_back(task, &global_queue);
        }

        assert_eq!(
            local_queue.len() as usize,
            LOCAL_QUEUE_CAP - LOCAL_QUEUE_CAP / 2
        );
        assert_eq!(global_queue.len.load(Acquire), 1 + LOCAL_QUEUE_CAP / 2);
    }

    /// InnerBuffer::pop_front() UT test cases
    ///
    /// # Brief
    /// case execution
    /// 1. Multi-threaded take out task operation with empty local queue, check
    ///    if the function is correct
    /// 2. If the local queue is not empty, multi-threaded take out operations
    ///    up to the number of existing tasks and check if the function is
    ///    correct
    /// 3. If the local queue is not empty, the multi-threaded operation to take
    ///    out more than the number of existing tasks, check whether the
    ///    function is correct should be related to the entry after the
    ///    initialization is completed
    #[test]
    fn ut_inner_buffer_pop_front() {
        // 1. Multi-threaded take out task operation with empty local queue, check if
        // the function is correct
        let local_queue = LocalQueue::new();
        let global_queue = GlobalQueue::new();
        assert!(local_queue.pop_front().is_none());

        // 2. If the local queue is not empty, multi-threaded take out operations up to
        // the number of existing tasks and check if the function is correct
        let local_queue = Arc::new(LocalQueue::new());
        let builder = TaskBuilder::new();

        let (arc_handle, _) = Driver::initialize();

        for _ in 0..LOCAL_QUEUE_CAP {
            let exe_scheduler =
                Arc::downgrade(&Arc::new(MultiThreadScheduler::new(2, arc_handle.clone())));
            let (task, _) = create_task(
                &builder,
                exe_scheduler,
                test_future(),
                VirtualTableType::Ylong,
            );
            local_queue.push_back(task, &global_queue);
        }
        assert_eq!(local_queue.len(), LOCAL_QUEUE_CAP as u16);

        let local_queue_clone_one = local_queue.clone();
        let local_queue_clone_two = local_queue.clone();

        let thread_one = std::thread::spawn(move || {
            for _ in 0..LOCAL_QUEUE_CAP / 2 {
                local_queue_clone_one.pop_front_and_release();
            }
        });

        let thread_two = std::thread::spawn(move || {
            for _ in 0..LOCAL_QUEUE_CAP / 2 {
                local_queue_clone_two.pop_front_and_release();
            }
        });

        thread_one.join().expect("failed");
        thread_two.join().expect("failed");
        assert!(local_queue.is_empty());

        // 3. If the local queue is not empty, the multi-threaded operation to take out
        // more than the number of existing tasks, check whether the function is correct
        let local_queue = Arc::new(LocalQueue::new());

        let (arc_handle, _) = Driver::initialize();

        for _ in 0..LOCAL_QUEUE_CAP {
            let exe_scheduler =
                Arc::downgrade(&Arc::new(MultiThreadScheduler::new(2, arc_handle.clone())));
            let (task, _) = create_task(
                &builder,
                exe_scheduler,
                test_future(),
                VirtualTableType::Ylong,
            );
            local_queue.push_back(task, &global_queue);
        }
        assert_eq!(local_queue.len(), LOCAL_QUEUE_CAP as u16);

        let local_queue_clone_one = local_queue.clone();
        let local_queue_clone_two = local_queue.clone();

        let thread_one = std::thread::spawn(move || {
            for _ in 0..LOCAL_QUEUE_CAP {
                local_queue_clone_one.pop_front_and_release();
            }
        });

        let thread_two = std::thread::spawn(move || {
            for _ in 0..LOCAL_QUEUE_CAP {
                local_queue_clone_two.pop_front_and_release();
            }
        });

        thread_one.join().expect("failed");
        thread_two.join().expect("failed");
        assert!(local_queue.is_empty());
    }

    /// InnerBuffer::steal_into() UT test cases
    ///
    /// # Brief
    /// case execution
    /// 1. In the single-threaded case, the number of tasks already in the local
    ///    queue is not more than half, steal from other local queues, the
    ///    number of steals is 0, check whether the function is completed
    #[test]
    fn ut_inner_buffer_steal_into_zero() {
        let local_queue = LocalQueue::new();
        let other_local_queue = LocalQueue::new();

        assert!(other_local_queue.steal_into(&local_queue).is_none());
    }

    /// InnerBuffer::steal_into() UT test cases
    ///
    /// # Brief
    /// case execution
    /// 1. In the single-threaded case, the number of tasks already in the local
    ///    queue is not more than half, steal from other local queues, the
    ///    number of steals is not 0, check whether the function is completed
    #[test]
    fn ut_inner_buffer_steal_into_less_than_half() {
        let builder = TaskBuilder::new();
        let (arc_handle, _) = Driver::initialize();
        let multi_scheduler = Arc::new(MultiThreadScheduler::new(1, arc_handle));

        let local_queue = LocalQueue::new();
        let other_local_queue = LocalQueue::new();
        let global_queue = GlobalQueue::new();

        for _ in 0..LOCAL_QUEUE_CAP {
            let exe_scheduler = Arc::downgrade(&multi_scheduler);
            let (task, _) = create_task(
                &builder,
                exe_scheduler,
                test_future(),
                VirtualTableType::Ylong,
            );
            other_local_queue.push_back(task, &global_queue);
        }

        other_local_queue.steal_into_and_release(&local_queue);

        assert_eq!(other_local_queue.len(), (LOCAL_QUEUE_CAP / 2) as u16);
        assert_eq!(local_queue.len(), (LOCAL_QUEUE_CAP / 2 - 1) as u16);
    }

    /// InnerBuffer::steal_into() UT test cases
    ///
    /// # Brief
    /// case execution
    /// 1. Multi-threaded case, other queues are doing take out operations, but
    ///    steal from this queue to see if the function is completed
    #[test]
    fn ut_inner_buffer_steal_into_multi_thread() {
        let builder = TaskBuilder::new();
        let (arc_handle, _) = Driver::initialize();
        let multi_scheduler = Arc::new(MultiThreadScheduler::new(1, arc_handle));

        let local_queue = Arc::new(LocalQueue::new());
        let local_queue_clone = local_queue.clone();

        let other_local_queue = Arc::new(LocalQueue::new());
        let other_local_queue_clone_one = other_local_queue.clone();
        let other_local_queue_clone_two = other_local_queue.clone();

        let global_queue = GlobalQueue::new();
        for _ in 0..LOCAL_QUEUE_CAP {
            let exe_scheduler = Arc::downgrade(&multi_scheduler);
            let (task, _) = create_task(
                &builder,
                exe_scheduler,
                test_future(),
                VirtualTableType::Ylong,
            );
            other_local_queue.push_back(task, &global_queue);
        }

        let thread_one = std::thread::spawn(move || {
            for _ in 0..LOCAL_QUEUE_CAP / 2 {
                other_local_queue_clone_one.pop_front_and_release();
            }
        });

        let thread_two = std::thread::spawn(move || {
            other_local_queue_clone_two.steal_into_and_release(&local_queue_clone);
        });

        thread_one.join().expect("failed");
        thread_two.join().expect("failed");

        assert_eq!(
            other_local_queue.len() + local_queue.len() + 1,
            (LOCAL_QUEUE_CAP / 2) as u16
        );
    }

    /// InnerBuffer::steal_into() UT test cases
    ///
    /// # Brief
    /// case execution
    /// 1. In the multi-threaded case, other queues are being stolen by
    ///    non-local queues, steal from that stolen queue and see if the
    ///    function is completed invalid value, and the property value should be
    ///    related to the entry after the initialization is completed
    #[test]
    fn ut_inner_buffer_steal_into_multi_threaded_complex() {
        let global_queue = GlobalQueue::new();

        let builder = TaskBuilder::new();
        let (arc_handle, _) = Driver::initialize();
        let multi_scheduler = Arc::new(MultiThreadScheduler::new(1, arc_handle));

        let local_queue_one = Arc::new(LocalQueue::new());
        let local_queue_one_clone = local_queue_one.clone();

        let local_queue_two = Arc::new(LocalQueue::new());
        let local_queue_two_clone = local_queue_two.clone();

        let other_local_queue = Arc::new(LocalQueue::new());
        let other_local_queue_clone_one = other_local_queue.clone();
        let other_local_queue_clone_two = other_local_queue.clone();

        for _ in 0..LOCAL_QUEUE_CAP {
            let exe_scheduler = Arc::downgrade(&multi_scheduler);
            let (task, _) = create_task(
                &builder,
                exe_scheduler,
                test_future(),
                VirtualTableType::Ylong,
            );
            other_local_queue.push_back(task, &global_queue);
        }

        let thread_one = std::thread::spawn(move || {
            park();
            other_local_queue_clone_one.steal_into_and_release(&local_queue_one_clone);
        });

        let thread_two = std::thread::spawn(move || {
            other_local_queue_clone_two.steal_into_and_release(&local_queue_two_clone);
        });

        thread_two.join().expect("failed");
        thread_one.thread().unpark();
        thread_one.join().expect("failed");

        assert_eq!(local_queue_two.len(), (LOCAL_QUEUE_CAP / 2 - 1) as u16);
        assert_eq!(local_queue_one.len(), (LOCAL_QUEUE_CAP / 4 - 1) as u16);
    }

    /// InnerBuffer::steal_into() UT test cases
    ///
    /// # Brief
    /// case execution
    /// 1. In the single-threaded case, the local queue has more than half the
    ///    number of tasks, steal from other local queues, the number of steals
    ///    is 0, check whether the function is completed
    #[test]
    fn ut_inner_buffer_steal_into_more_than_half() {
        // 1. In the single-threaded case, the local queue has more than half the number
        // of tasks, steal from other local queues, the number of steals is 0, check
        // whether the function is completed
        let local_queue = LocalQueue::new();
        let other_local_queue = LocalQueue::new();
        let global_queue = GlobalQueue::new();

        let builder = TaskBuilder::new();
        let (arc_handle, _) = Driver::initialize();
        let multi_scheduler = Arc::new(MultiThreadScheduler::new(1, arc_handle));

        for _ in 0..LOCAL_QUEUE_CAP {
            let exe_scheduler = Arc::downgrade(&multi_scheduler);
            let (task, _) = create_task(
                &builder,
                exe_scheduler,
                test_future(),
                VirtualTableType::Ylong,
            );
            local_queue.push_back(task, &global_queue);
        }

        for _ in 0..LOCAL_QUEUE_CAP {
            let exe_scheduler = Arc::downgrade(&multi_scheduler);
            let (task, _) = create_task(
                &builder,
                exe_scheduler,
                test_future(),
                VirtualTableType::Ylong,
            );
            other_local_queue.push_back(task, &global_queue);
        }

        assert!(other_local_queue.steal_into(&local_queue).is_none());
    }
}

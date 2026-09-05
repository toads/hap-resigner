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

use std::collections::VecDeque;
use std::future::Future;
use std::mem;
use std::pin::Pin;
#[cfg(feature = "metrics")]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::{AcqRel, Acquire};
use std::sync::{Arc, Condvar, Mutex};
use std::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

use crate::executor::driver::{Driver, Handle, ParkFlag};
use crate::executor::Schedule;
use crate::task::{JoinHandle, Task, TaskBuilder, VirtualTableType};

// Idle state
const IDLE: usize = 0;
// Suspended on condvar
const PARKED_ON_CONDVAR: usize = 1;
// Suspended on driver
const PARKED_ON_DRIVER: usize = 2;
// notified by the spawned task
const NOTIFIED: usize = 3;
// notified by the blocked task
const NOTIFIED_BLOCK: usize = 4;

pub(crate) struct CurrentThreadSpawner {
    pub(crate) scheduler: Arc<CurrentThreadScheduler>,
    pub(crate) driver: Arc<Mutex<Driver>>,
    pub(crate) handle: Arc<Handle>,
}

#[derive(Default)]
pub(crate) struct CurrentThreadScheduler {
    pub(crate) inner: Mutex<VecDeque<Task>>,
    pub(crate) parker_list: Mutex<Vec<Arc<Parker>>>,
    /// Total task count
    #[cfg(feature = "metrics")]
    pub(crate) count: AtomicU64,
}

unsafe impl Sync for CurrentThreadScheduler {}

impl Schedule for CurrentThreadScheduler {
    #[inline]
    fn schedule(&self, task: Task, _lifo: bool) {
        let mut queue = self.inner.lock().unwrap();
        #[cfg(feature = "metrics")]
        self.count.fetch_add(1, AcqRel);
        queue.push_back(task);

        let parker_list = self.parker_list.lock().unwrap();
        for parker in &*parker_list {
            parker.unpark(false);
        }
    }
}

impl CurrentThreadScheduler {
    fn pop(&self) -> Option<Task> {
        let mut queue = self.inner.lock().unwrap();
        queue.pop_front()
    }
}

pub(crate) struct Parker {
    state: AtomicUsize,
    mutex: Mutex<bool>,
    condvar: Condvar,
    driver: Arc<Mutex<Driver>>,
    handle: Arc<Handle>,
}

impl Parker {
    fn new(driver: Arc<Mutex<Driver>>, handle: Arc<Handle>) -> Parker {
        Parker {
            state: AtomicUsize::new(IDLE),
            mutex: Mutex::new(false),
            condvar: Condvar::new(),
            driver,
            handle,
        }
    }

    fn park(&self) -> bool {
        let (mut park, mut wake) = (true, false);
        if let Ok(mut driver) = self.driver.try_lock() {
            (park, wake) = self.park_on_driver(&mut driver);
        }
        if park {
            self.park_on_condvar()
        } else {
            wake
        }
    }

    fn park_on_driver(&self, driver: &mut Driver) -> (bool, bool) {
        match self
            .state
            .compare_exchange(IDLE, PARKED_ON_DRIVER, AcqRel, Acquire)
        {
            Ok(_) => {}
            Err(NOTIFIED_BLOCK) | Err(NOTIFIED) => {
                return match self.state.swap(IDLE, AcqRel) {
                    // No need to park on condvar, need to awaken the blocked task.
                    NOTIFIED_BLOCK => (false, true),
                    // No need to park on condvar, no need to awaken the blocked task.
                    NOTIFIED => (false, false),
                    actual => panic!("invalid park state when notifying; actual = {actual}"),
                };
            }
            Err(actual) => panic!("inconsistent park state; actual = {actual}"),
        }

        let park = match driver.run() {
            ParkFlag::NotPark => false,
            ParkFlag::Park => true,
            ParkFlag::ParkTimeout(_) => false,
        };

        match self.state.swap(IDLE, AcqRel) {
            NOTIFIED => (false, false),
            NOTIFIED_BLOCK => (false, true),
            PARKED_ON_DRIVER => (park, false),
            n => panic!("inconsistent park_timeout state: {n}"),
        }
    }

    fn park_on_condvar(&self) -> bool {
        let mut lock = self.mutex.lock().unwrap();
        match self
            .state
            .compare_exchange(IDLE, PARKED_ON_CONDVAR, AcqRel, Acquire)
        {
            Ok(_) => {}
            Err(NOTIFIED_BLOCK) | Err(NOTIFIED) => {
                return match self.state.swap(IDLE, AcqRel) {
                    // Need to awaken the blocked task.
                    NOTIFIED_BLOCK => true,
                    // No need to awaken the blocked task.
                    NOTIFIED => false,
                    actual => panic!("invalid park state when notifying; actual = {actual}"),
                };
            }
            Err(actual) => panic!("inconsistent park state; actual = {actual}"),
        }

        while !*lock {
            lock = self.condvar.wait(lock).unwrap();
        }
        *lock = false;

        match self.state.swap(IDLE, AcqRel) {
            NOTIFIED => false,
            NOTIFIED_BLOCK => true,
            n => panic!("inconsistent park_timeout state: {n}"),
        }
    }

    fn unpark(&self, wake: bool) {
        if wake {
            match self.state.swap(NOTIFIED_BLOCK, AcqRel) {
                IDLE | NOTIFIED | NOTIFIED_BLOCK => {}
                PARKED_ON_CONDVAR => {
                    let mut lock = self.mutex.lock().unwrap();
                    *lock = true;
                    mem::drop(lock);
                    self.condvar.notify_one();
                }
                PARKED_ON_DRIVER => self.handle.wake(),
                actual => panic!("inconsistent state in unpark; actual = {actual}"),
            }
        } else {
            match self.state.swap(NOTIFIED, AcqRel) {
                IDLE | NOTIFIED => {}
                NOTIFIED_BLOCK => self.unpark(true),
                PARKED_ON_CONDVAR => {
                    let mut lock = self.mutex.lock().unwrap();
                    *lock = true;
                    mem::drop(lock);
                    self.condvar.notify_one();
                }
                PARKED_ON_DRIVER => self.handle.wake(),
                actual => panic!("inconsistent state in unpark; actual = {actual}"),
            }
        }
    }
}

fn waker(parker: Arc<Parker>) -> Waker {
    let data = Arc::into_raw(parker).cast::<()>();
    unsafe { Waker::from_raw(RawWaker::new(data, &CURRENT_THREAD_RAW_WAKER_VIRTUAL_TABLE)) }
}

static CURRENT_THREAD_RAW_WAKER_VIRTUAL_TABLE: RawWakerVTable =
    RawWakerVTable::new(clone, wake, wake_by_ref, drop);

fn clone(ptr: *const ()) -> RawWaker {
    let parker = unsafe { Arc::from_raw(ptr.cast::<Parker>()) };

    // increment the ref count
    mem::forget(parker.clone());

    let data = Arc::into_raw(parker).cast::<()>();
    RawWaker::new(data, &CURRENT_THREAD_RAW_WAKER_VIRTUAL_TABLE)
}

fn wake(ptr: *const ()) {
    let parker = unsafe { Arc::from_raw(ptr.cast::<Parker>()) };
    parker.unpark(true);
}

fn wake_by_ref(ptr: *const ()) {
    let parker = unsafe { Arc::from_raw(ptr.cast::<Parker>()) };
    parker.unpark(true);
    mem::forget(parker);
}

fn drop(ptr: *const ()) {
    unsafe { mem::drop(Arc::from_raw(ptr.cast::<Parker>())) };
}

impl CurrentThreadSpawner {
    pub(crate) fn new() -> Self {
        let (handle, driver) = Driver::initialize();
        Self {
            scheduler: Default::default(),
            driver,
            handle,
        }
    }

    fn get_parker(&self) -> Parker {
        Parker::new(self.driver.clone(), self.handle.clone())
    }

    pub(crate) fn spawn<T>(&self, builder: &TaskBuilder, task: T) -> JoinHandle<T::Output>
    where
        T: Future + Send + 'static,
        T::Output: Send + 'static,
    {
        let scheduler = Arc::downgrade(&self.scheduler);
        let (task, handle) = Task::create_task(builder, scheduler, task, VirtualTableType::Ylong);

        let mut queue = self.scheduler.inner.lock().unwrap();
        queue.push_back(task);
        #[cfg(feature = "metrics")]
        self.scheduler.count.fetch_add(1, AcqRel);

        let parker_list = self.scheduler.parker_list.lock().unwrap();
        for parker in &*parker_list {
            parker.unpark(false);
        }
        handle
    }

    pub(crate) fn block_on<T>(&self, future: T) -> T::Output
    where
        T: Future,
    {
        let parker = Arc::new(self.get_parker());
        let mut parker_list = self.scheduler.parker_list.lock().unwrap();
        parker_list.push(parker.clone());
        mem::drop(parker_list);

        let waker = waker(parker.clone());
        let mut cx = Context::from_waker(&waker);

        let mut future = future;
        let mut future = unsafe { Pin::new_unchecked(&mut future) };

        let mut wake = true;

        loop {
            if wake {
                if let Poll::Ready(res) = future.as_mut().poll(&mut cx) {
                    return res;
                }
            }

            while let Some(task) = self.scheduler.pop() {
                task.run();
            }

            wake = parker.park();
        }
    }
}

#[cfg(test)]
mod test {
    macro_rules! cfg_sync {
        ($($item:item)*) => {
            $(
                #[cfg(feature = "sync")]
                $item
            )*
        }
    }

    use crate::executor::current_thread::CurrentThreadSpawner;
    use crate::task::{yield_now, TaskBuilder};

    cfg_sync! {
        use crate::sync::Waiter;
        use std::sync::atomic::AtomicUsize;
        use std::sync::atomic::Ordering::{Acquire, Release};
        use std::sync::{Condvar, Mutex};
        use std::sync::Arc;

        pub(crate) struct Parker {
            mutex: Mutex<bool>,
            condvar: Condvar,
        }

        impl Parker {
            fn new() -> Parker {
                Parker {
                    mutex: Mutex::new(false),
                    condvar: Condvar::new(),
                }
            }

            fn notified(&self) {
                let mut guard = self.mutex.lock().unwrap();

                while !*guard {
                    guard = self.condvar.wait(guard).unwrap();
                }
                *guard = false;
            }

            fn notify_one(&self) {
                let mut guard = self.mutex.lock().unwrap();
                *guard = true;
                drop(guard);
                self.condvar.notify_one();
            }
        }
    }

    cfg_net! {
        use std::net::SocketAddr;
        use crate::net::{TcpListener, TcpStream};
        use crate::io::{AsyncReadExt, AsyncWriteExt};

        const ADDR: &str = "127.0.0.1:0";

        pub async fn ylong_tcp_server(tx: crate::sync::oneshot::Sender<SocketAddr>) {
            let tcp = TcpListener::bind(ADDR).await.unwrap();
            let addr = tcp.local_addr().unwrap();
            tx.send(addr).unwrap();
            let (mut stream, _) = tcp.accept().await.unwrap();
            for _ in 0..3 {
                let mut buf = [0; 100];
                stream.read_exact(&mut buf).await.unwrap();
                assert_eq!(buf, [3; 100]);

                let buf = [2; 100];
                stream.write_all(&buf).await.unwrap();
            }
        }

        pub async fn ylong_tcp_client(rx: crate::sync::oneshot::Receiver<SocketAddr>) {
            let addr = rx.await.unwrap();
            let mut tcp = TcpStream::connect(addr).await;
            while tcp.is_err() {
                tcp = TcpStream::connect(addr).await;
            }
            let mut tcp = tcp.unwrap();
            for _ in 0..3 {
                let buf = [3; 100];
                tcp.write_all(&buf).await.unwrap();

                let mut buf = [0; 100];
                tcp.read_exact(&mut buf).await.unwrap();
                assert_eq!(buf, [2; 100]);
            }
        }
    }

    /// UT test cases for `block_on()`.
    ///
    /// # Brief
    /// 1. Spawn two tasks, check the running status of tasks in the queue when
    ///    the yield task is blocked on.
    #[test]
    fn ut_current_thread_block_on() {
        let spawner = CurrentThreadSpawner::new();
        let handle1 = spawner.spawn(&TaskBuilder::default(), async move { 1 });
        let handle2 = spawner.spawn(&TaskBuilder::default(), async move { 1 });
        spawner.block_on(yield_now());
        assert_eq!(spawner.scheduler.inner.lock().unwrap().len(), 0);
        assert_eq!(spawner.block_on(handle1).unwrap(), 1);
        assert_eq!(spawner.block_on(handle2).unwrap(), 1);
    }

    /// UT test cases for `spawn()` and `block_on()`.
    ///
    /// # Brief
    /// 1. Spawn two tasks before the blocked task running and check the status
    ///    of two tasks.
    /// 2. Spawn two tasks after the blocked task running and check the status
    ///    of two tasks.
    #[test]
    #[cfg(feature = "sync")]
    fn ut_current_thread_run_queue() {
        use crate::builder::RuntimeBuilder;
        let spawner = Arc::new(RuntimeBuilder::new_current_thread().build().unwrap());

        let finished = Arc::new(AtomicUsize::new(0));

        let finished_clone = finished.clone();
        let notify1 = Arc::new(Parker::new());
        let notify1_clone = notify1.clone();
        spawner.spawn(async move {
            finished_clone.fetch_add(1, Release);
            notify1_clone.notify_one();
        });

        let finished_clone = finished.clone();
        let notify2 = Arc::new(Parker::new());
        let notify2_clone = notify2.clone();
        spawner.spawn(async move {
            finished_clone.fetch_add(1, Release);
            notify2_clone.notify_one();
        });

        let waiter = Arc::new(Waiter::new());
        let waiter_clone = waiter.clone();
        let spawner_clone = spawner.clone();
        let join = std::thread::spawn(move || {
            spawner_clone.block_on(async move { waiter_clone.wait().await })
        });

        notify1.notified();
        notify2.notified();
        assert_eq!(finished.load(Acquire), 2);

        let finished_clone = finished.clone();
        let notify1 = Arc::new(Parker::new());
        let notify1_clone = notify1.clone();
        spawner.spawn(async move {
            finished_clone.fetch_add(1, Release);
            notify1_clone.notify_one();
        });

        let finished_clone = finished.clone();
        let notify2 = Arc::new(Parker::new());
        let notify2_clone = notify2.clone();
        spawner.spawn(async move {
            finished_clone.fetch_add(1, Release);
            notify2_clone.notify_one();
        });

        notify1.notified();
        notify2.notified();
        assert_eq!(finished.load(Acquire), 4);

        waiter.wake_one();
        join.join().unwrap();

        #[cfg(feature = "net")]
        crate::executor::worker::CURRENT_WORKER.with(|ctx| {
            ctx.set(std::ptr::null());
        });
    }

    /// UT test cases for io tasks.
    ///
    /// # Brief
    /// 1. Spawns a tcp server to read and write data for three times.
    /// 2. Spawns a tcp client to read and write data for three times.
    #[test]
    #[cfg(feature = "net")]
    fn ut_current_thread_io() {
        use crate::builder::RuntimeBuilder;

        let spawner = RuntimeBuilder::new_current_thread().build().unwrap();
        let (tx, rx) = crate::sync::oneshot::channel();
        let join_handle = spawner.spawn(ylong_tcp_server(tx));

        spawner.block_on(ylong_tcp_client(rx));
        spawner.block_on(join_handle).unwrap();

        let spawner = RuntimeBuilder::new_current_thread().build().unwrap();
        let (tx, rx) = crate::sync::oneshot::channel();
        let join_handle = spawner.spawn(ylong_tcp_client(rx));
        spawner.block_on(ylong_tcp_server(tx));
        spawner.block_on(join_handle).unwrap();
    }
}

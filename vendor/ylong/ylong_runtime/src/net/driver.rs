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
use std::ops::Deref;
#[cfg(feature = "metrics")]
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use ylong_io::{Interest, Source, Token};

use crate::net::{Ready, ScheduleIO, Tick};
use crate::util::bit::{Bit, Mask};
use crate::util::slab::{Address, Ref, Slab};

cfg_ffrt! {
    #[cfg(all(feature = "signal", target_os = "linux"))]
    use crate::signal::unix::SignalDriver;
    use libc::{c_void, c_int, c_uint, c_uchar};
}

cfg_not_ffrt! {
    use ylong_io::{Events, Poll};
    use std::time::Duration;

    const EVENTS_MAX_CAPACITY: usize = 1024;
    const WAKE_TOKEN: Token = Token(1 << 31);
}

#[cfg(all(feature = "signal", target_family = "unix"))]
pub(crate) const SIGNAL_TOKEN: Token = Token((1 << 31) + 1);
const DRIVER_TICK_INIT: u8 = 0;

// Token structure
// | reserved | generation | address |
// |----------|------------|---------|
// |   1 bit  |   7 bits   | 24 bits |
const GENERATION: Mask = Mask::new(7, 24);
const ADDRESS: Mask = Mask::new(24, 0);

/// IO reactor that listens to fd events and wakes corresponding tasks.
pub(crate) struct IoDriver {
    /// Stores every IO source that is ready
    resources: Option<Slab<ScheduleIO>>,

    /// Counter used for slab struct to compact
    tick: u8,

    /// Used for epoll
    #[cfg(not(feature = "ffrt"))]
    poll: Arc<Poll>,

    /// Stores IO events that need to be handled
    #[cfg(not(feature = "ffrt"))]
    events: Option<Events>,

    /// Indicate if there is a signal coming
    #[cfg(all(not(feature = "ffrt"), feature = "signal", target_family = "unix"))]
    signal_pending: bool,

    /// Save Handle used in metrics.
    #[cfg(feature = "metrics")]
    io_handle_inner: Arc<Inner>,
}

pub(crate) struct IoHandle {
    inner: Arc<Inner>,
    #[cfg(not(feature = "ffrt"))]
    pub(crate) waker: ylong_io::Waker,
}

cfg_ffrt!(
    use std::mem::MaybeUninit;
    static mut DRIVER: MaybeUninit<IoDriver> = MaybeUninit::uninit();
    static mut HANDLE: MaybeUninit<IoHandle> = MaybeUninit::uninit();
);

#[cfg(feature = "ffrt")]
impl IoHandle {
    fn new(inner: Arc<Inner>) -> Self {
        IoHandle { inner }
    }

    pub(crate) fn get_ref() -> &'static Self {
        IoDriver::initialize();
        unsafe { &*HANDLE.as_ptr() }
    }
}

#[cfg(not(feature = "ffrt"))]
impl IoHandle {
    fn new(inner: Arc<Inner>, waker: ylong_io::Waker) -> Self {
        IoHandle { inner, waker }
    }

    #[cfg(feature = "metrics")]
    pub(crate) fn get_registered_count(&self) -> u64 {
        self.inner
            .metrics
            .registered_count
            .load(std::sync::atomic::Ordering::Acquire)
    }

    #[cfg(feature = "metrics")]
    pub(crate) fn get_ready_count(&self) -> u64 {
        self.inner
            .metrics
            .ready_count
            .load(std::sync::atomic::Ordering::Acquire)
    }
}

impl Deref for IoHandle {
    type Target = Arc<Inner>;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

/// In charge of two functionalities
///
/// 1）IO registration
/// 2）Resource management
pub(crate) struct Inner {
    /// When the driver gets dropped, the resources in the driver will be
    /// transmitted to here. Then all the slabs inside will get dropped when
    /// Inner's ref count clears to zero, so there is no concurrent problem
    /// when new slabs gets inserted
    resources: Mutex<Option<Slab<ScheduleIO>>>,

    /// Used to register scheduleIO into the slab
    allocator: Slab<ScheduleIO>,

    /// Used to register fd
    #[cfg(not(feature = "ffrt"))]
    registry: Arc<Poll>,

    /// Metrics
    #[cfg(feature = "metrics")]
    metrics: InnerMetrics,
}

/// Metrics of Inner
#[cfg(feature = "metrics")]
struct InnerMetrics {
    /// Fd registered count. This value will only increment, not decrease.
    registered_count: AtomicU64,

    /// Ready events count. This value will only increment, not decrease.
    ready_count: AtomicU64,
}

impl IoDriver {
    /// IO dispatch function. Wakes the task through the token getting from the
    /// epoll events.
    fn dispatch(&mut self, token: Token, ready: Ready) {
        let addr_bit = Bit::from_usize(token.0);
        let addr = addr_bit.get_by_mask(ADDRESS);

        // IoDriver at this point has been initialized, therefore resources must be some
        let io = match self
            .resources
            .as_mut()
            .unwrap()
            .get(Address::from_usize(addr))
        {
            Some(io) => io,
            None => return,
        };

        if io
            .set_readiness(Some(token.0), Tick::Set(self.tick), |curr| curr | ready)
            .is_err()
        {
            return;
        }

        // Wake the io task
        io.wake(ready)
    }
}

#[cfg(not(feature = "ffrt"))]
impl IoDriver {
    pub(crate) fn initialize() -> (IoHandle, IoDriver) {
        let poll =
            Poll::new().unwrap_or_else(|e| panic!("IO poller initialize failed, error: {e}"));
        let waker = ylong_io::Waker::new(&poll, WAKE_TOKEN)
            .unwrap_or_else(|e| panic!("ylong_io waker construction failed, error: {e}"));
        let arc_poll = Arc::new(poll);
        let events = Events::with_capacity(EVENTS_MAX_CAPACITY);
        let slab = Slab::new();
        let allocator = slab.handle();
        let inner = Arc::new(Inner {
            resources: Mutex::new(None),
            allocator,
            registry: arc_poll.clone(),
            #[cfg(feature = "metrics")]
            metrics: InnerMetrics {
                registered_count: AtomicU64::new(0),
                ready_count: AtomicU64::new(0),
            },
        });

        let driver = IoDriver {
            resources: Some(slab),
            events: Some(events),
            tick: DRIVER_TICK_INIT,
            poll: arc_poll,
            #[cfg(feature = "metrics")]
            io_handle_inner: inner.clone(),
            #[cfg(all(feature = "signal", target_family = "unix"))]
            signal_pending: false,
        };

        (IoHandle::new(inner, waker), driver)
    }

    /// Runs the driver. This method will blocking wait for fd events to come in
    /// and then wakes the corresponding tasks through the events.
    ///
    /// In linux environment, the driver uses epoll.
    pub(crate) fn drive(&mut self, time_out: Option<Duration>) -> io::Result<bool> {
        use ylong_io::EventTrait;

        // For every 255 ticks, cleans the redundant entries inside the slab
        const COMPACT_INTERVAL: u8 = 255;

        self.tick = self.tick.wrapping_add(1);

        if self.tick == COMPACT_INTERVAL {
            unsafe {
                // IoDriver at this point has been initialized, therefore resources must be some
                self.resources.as_mut().unwrap().compact();
            }
        }

        let mut events = match self.events.take() {
            Some(ev) => ev,
            None => {
                let err = io::Error::new(io::ErrorKind::Other, "driver event store missing.");
                return Err(err);
            }
        };
        match self.poll.poll(&mut events, time_out) {
            Ok(_) => {}
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(err) => return Err(err),
        }
        let has_events = !events.is_empty();

        for event in events.iter() {
            let token = event.token();
            if token == WAKE_TOKEN {
                continue;
            }
            #[cfg(all(feature = "signal", target_family = "unix"))]
            if token == SIGNAL_TOKEN {
                self.signal_pending = true;
                continue;
            }
            let ready = Ready::from_event(event);
            self.dispatch(token, ready);
        }
        #[cfg(feature = "metrics")]
        self.io_handle_inner
            .metrics
            .ready_count
            .fetch_add(events.len() as u64, std::sync::atomic::Ordering::AcqRel);

        self.events = Some(events);
        Ok(has_events)
    }

    #[cfg(all(feature = "signal", target_family = "unix"))]
    pub(crate) fn process_signal(&mut self) -> bool {
        let pending = self.signal_pending;
        self.signal_pending = false;
        pending
    }
}

#[cfg(feature = "ffrt")]
impl IoDriver {
    fn initialize() {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| unsafe {
            let slab = Slab::new();
            let allocator = slab.handle();
            let inner = Arc::new(Inner {
                resources: Mutex::new(None),
                allocator,
            });

            let driver = IoDriver {
                resources: Some(slab),
                tick: DRIVER_TICK_INIT,
            };
            HANDLE = MaybeUninit::new(IoHandle::new(inner));
            DRIVER = MaybeUninit::new(driver);
        });
    }

    /// Initializes the single instance IO driver.
    pub(crate) fn get_mut_ref() -> &'static mut IoDriver {
        IoDriver::initialize();
        unsafe { &mut *DRIVER.as_mut_ptr() }
    }
}

#[cfg(all(feature = "ffrt", feature = "signal", target_os = "linux"))]
extern "C" fn ffrt_dispatch_signal_event(data: *const c_void, _ready: c_uint, _new_tick: c_uchar) {
    let token = Token::from_usize(data as usize);
    if token == SIGNAL_TOKEN {
        SignalDriver::get_mut_ref().broadcast();
        #[cfg(feature = "process")]
        crate::process::GlobalZombieChild::get_instance().release_zombie();
    }
}

#[cfg(feature = "ffrt")]
extern "C" fn ffrt_dispatch_event(data: *const c_void, ready: c_uint, new_tick: c_uchar) {
    const COMPACT_INTERVAL: u8 = 255;

    let driver = IoDriver::get_mut_ref();

    if new_tick == COMPACT_INTERVAL && driver.tick != new_tick {
        unsafe {
            driver.resources.as_mut().unwrap().compact();
        }
    }
    driver.tick = new_tick;

    let token = Token::from_usize(data as usize);
    let ready = crate::net::ready::from_event_inner(ready as i32);
    driver.dispatch(token, ready);
}

impl Inner {
    fn allocate_schedule_io_pair(&self) -> io::Result<(Ref<ScheduleIO>, usize)> {
        let (addr, schedule_io) = unsafe {
            self.allocator.allocate().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::Other,
                    "driver at max registered I/O resources.",
                )
            })?
        };
        let mut base = Bit::from_usize(0);
        base.set_by_mask(GENERATION, schedule_io.generation());
        base.set_by_mask(ADDRESS, addr.as_usize());
        Ok((schedule_io, base.as_usize()))
    }
}

#[cfg(not(feature = "ffrt"))]
impl Inner {
    #[cfg(all(feature = "signal", target_family = "unix"))]
    pub(crate) fn register_source_with_token(
        &self,
        io: &mut impl Source,
        token: Token,
        interest: Interest,
    ) -> io::Result<()> {
        self.registry.register(io, token, interest)
    }

    /// Registers the fd of the `Source` object
    pub(crate) fn register_source(
        &self,
        io: &mut impl Source,
        interest: Interest,
    ) -> io::Result<Ref<ScheduleIO>> {
        // Allocates space for the slab. If reaches maximum capacity, error will be
        // returned
        let (schedule_io, token) = self.allocate_schedule_io_pair()?;

        self.registry
            .register(io, Token::from_usize(token), interest)?;
        #[cfg(feature = "metrics")]
        self.metrics
            .registered_count
            .fetch_add(1, std::sync::atomic::Ordering::AcqRel);
        Ok(schedule_io)
    }

    /// Deregisters the fd of the `Source` object.
    pub(crate) fn deregister_source(&self, io: &mut impl Source) -> io::Result<()> {
        self.registry.deregister(io)
    }
}

#[cfg(feature = "ffrt")]
impl Inner {
    #[cfg(all(feature = "signal", target_os = "linux"))]
    pub(crate) fn register_source_with_token(
        &self,
        io: &mut impl Source,
        token: Token,
        interest: Interest,
    ) {
        let event = interest.into_io_event();
        unsafe {
            ylong_ffrt::ffrt_poller_register(
                io.get_fd() as c_int,
                event,
                token.0 as *const c_void,
                ffrt_dispatch_signal_event,
            );
        }
    }

    /// Registers the fd of the `Source` object
    pub(crate) fn register_source(
        &self,
        io: &mut impl Source,
        interest: Interest,
    ) -> io::Result<Ref<ScheduleIO>> {
        // Allocates space for the slab. If reaches maximum capacity, error will be
        // returned
        let (schedule_io, token) = self.allocate_schedule_io_pair()?;

        let event = interest.into_io_event();
        unsafe {
            ylong_ffrt::ffrt_poller_register(
                io.get_fd() as c_int,
                event,
                token as *const c_void,
                ffrt_dispatch_event,
            );
        }

        Ok(schedule_io)
    }
}

impl Drop for Inner {
    fn drop(&mut self) {
        let resources = self.resources.lock().unwrap().take();

        if let Some(mut slab) = resources {
            slab.for_each(|io| {
                io.shutdown();
            });
        }
    }
}

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

use std::fmt::Error;
use std::mem;
use std::mem::MaybeUninit;
use std::ptr::{addr_of_mut, NonNull};
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering::Relaxed;
use std::task::Waker;
use std::time::Duration;

use crate::util::linked_list::{Link, LinkedList, Node};

// In a slots, the number of slot.
const SLOTS_NUM: usize = 64;

// In a levels, the number of level.
const LEVELS_NUM: usize = 6;

// Maximum sleep duration.
pub(crate) const MAX_DURATION: u64 = (1 << (6 * LEVELS_NUM)) - 1;

// Struct for timing and waking up corresponding tasks on the timing wheel.
pub(crate) struct Clock {
    // Expected expiration time.
    expiration: u64,

    // The level to which the clock will be inserted.
    level: usize,

    // Elapsed time duration.
    duration: u64,

    // The result obtained when the corresponding Sleep structure is woken up by
    // which can be used to determine if the Future is completed correctly.
    result: AtomicBool,

    // Corresponding waker,
    // which is used to wake up sleep coroutine.
    waker: Option<Waker>,

    // Linked_list node.
    node: Node<Clock>,
}

impl Clock {
    // Creates a default Clock structure.
    pub(crate) fn new() -> Self {
        Self {
            expiration: 0,
            level: 0,
            duration: 0,
            result: AtomicBool::new(false),
            waker: None,
            node: Node::new(),
        }
    }

    // Returns the expected expiration time.
    pub(crate) fn expiration(&self) -> u64 {
        self.expiration
    }

    // Sets the expected expiration time
    pub(crate) fn set_expiration(&mut self, expiration: u64) {
        self.expiration = expiration;
    }

    // Returns the level to which the clock will be inserted.
    pub(crate) fn level(&self) -> usize {
        self.level
    }

    // Sets the level to which the clock will be inserted.
    pub(crate) fn set_level(&mut self, level: usize) {
        self.level = level;
    }

    pub(crate) fn duration(&self) -> u64 {
        self.duration
    }

    pub(crate) fn set_duration(&mut self, duration: u64) {
        self.duration = duration;
    }

    // Returns the corresponding waker.
    pub(crate) fn take_waker(&mut self) -> Option<Waker> {
        self.waker.take()
    }

    // Sets the corresponding waker.
    pub(crate) fn set_waker(&mut self, waker: Waker) {
        self.waker = Some(waker);
    }

    // Returns the result.
    pub(crate) fn result(&self) -> bool {
        self.result.load(Relaxed)
    }

    // Sets the result.
    pub(crate) fn set_result(&mut self, result: bool) {
        self.result.store(result, Relaxed);
    }
}

impl Default for Clock {
    fn default() -> Self {
        Clock::new()
    }
}

unsafe impl Link for Clock {
    unsafe fn node(mut ptr: NonNull<Self>) -> NonNull<Node<Self>>
    where
        Self: Sized,
    {
        let node_ptr = addr_of_mut!(ptr.as_mut().node);
        NonNull::new_unchecked(node_ptr)
    }
}

pub(crate) enum TimeOut {
    ClockEntry(NonNull<Clock>),
    Duration(Duration),
    None,
}

pub(crate) struct Expiration {
    level: usize,
    slot: usize,
    deadline: u64,
}

pub(crate) struct Wheel {
    // Since the wheel started,
    // the number of milliseconds elapsed.
    elapsed: u64,

    // The time wheel levels are similar to a multi-layered dial.
    //
    // levels:
    //
    // 1  ms slots == 64 ms range
    // 64 ms slots ~= 4 sec range
    // 4 sec slots ~= 4 min range
    // 4 min slots ~= 4 hr range
    // 4 hr slots ~= 12 day range
    // 12 day slots ~= 2 yr range
    levels: Vec<Level>,

    // These corresponding timers have expired,
    // and are ready to be triggered.
    trigger: LinkedList<Clock>,
}

impl Wheel {
    // Creates a new timing wheel.
    pub(crate) fn new() -> Self {
        let levels = (0..LEVELS_NUM).map(Level::new).collect();

        Self {
            elapsed: 0,
            levels,
            trigger: Default::default(),
        }
    }

    // Return the elapsed.
    pub(crate) fn elapsed(&self) -> u64 {
        self.elapsed
    }

    // Set the elapsed.
    pub(crate) fn set_elapsed(&mut self, elapsed: u64) {
        self.elapsed = elapsed;
    }

    // Compare the timing wheel elapsed with the expiration,
    // from which to decide which level to insert.
    pub(crate) fn find_level(expiration: u64, elapsed: u64) -> usize {
        // 0011 1111
        const SLOT_MASK: u64 = (1 << 6) - 1;

        // Use the time difference value to find at which level.
        // Use XOR to determine the insertion of inspiration currently need to be
        // inserted into the level, using binary operations to determine which bit has
        // changed, and further determine which level. If don't use XOR, it will lead to
        // the re-insertion of the level of the calculation error, and insert to an
        // incorrect level.
        let mut masked = (expiration ^ elapsed) | SLOT_MASK;
        // 1111 1111 1111 1111 1111 1111 1111 1111 1111
        if masked >= MAX_DURATION {
            masked = MAX_DURATION - 1;
        }

        let leading_zeros = masked.leading_zeros() as usize;
        // Calculate how many valid bits there are.
        let significant = 63 - leading_zeros;

        // One level per 6 bit,
        // one slots has 2^6 slots.
        significant / 6
    }

    // Insert the corresponding TimerHandle into the specified position in the
    // timing wheel.
    pub(crate) fn insert(&mut self, mut clock_entry: NonNull<Clock>) -> Result<u64, Error> {
        let expiration = unsafe { clock_entry.as_ref().expiration() };

        if expiration <= self.elapsed() {
            // This means that the timeout period has passed,
            // and the time should be triggered immediately.
            return Err(Error);
        }

        let level = Self::find_level(expiration, self.elapsed());
        // Unsafe access to clock_entry is only unsafe when Sleep Drop,
        // `Sleep` here does not go into `Ready`.
        unsafe { clock_entry.as_mut().set_level(level) };
        self.levels[level].insert(clock_entry);
        Ok(expiration)
    }

    pub(crate) fn cancel(&mut self, clock_entry: NonNull<Clock>) {
        // Unsafe access to clock_entry is only unsafe when Sleep Drop,
        // `Sleep` here does not go into `Ready`.
        let level = unsafe { clock_entry.as_ref().level() };
        self.levels[level].cancel(clock_entry);
    }

    // Return where the next expiration is located, and its deadline.
    pub(crate) fn next_expiration(&self) -> Option<Expiration> {
        for level in 0..LEVELS_NUM {
            if let Some(expiration) = self.levels[level].next_expiration(self.elapsed()) {
                return Some(expiration);
            }
        }

        None
    }

    // Retrieve the corresponding expired TimerHandle.
    pub(crate) fn process_expiration(&mut self, expiration: &Expiration) {
        let mut handles = self.levels[expiration.level].take_slot(expiration.slot);
        while let Some(mut item) = handles.pop_back() {
            let expected_expiration = unsafe { item.as_ref().expiration() };
            if expected_expiration > expiration.deadline {
                let level = Self::find_level(expected_expiration, expiration.deadline);

                unsafe { item.as_mut().set_level(level) };

                self.levels[level].insert(item);
            } else {
                self.trigger.push_front(item);
            }
        }
    }

    // Determine which timers have timed out at the current time.
    pub(crate) fn poll(&mut self, now: u64) -> TimeOut {
        loop {
            if let Some(handle) = self.trigger.pop_back() {
                return TimeOut::ClockEntry(handle);
            }

            let expiration = self.next_expiration();

            match expiration {
                Some(ref expiration) if expiration.deadline > now => {
                    return TimeOut::Duration(Duration::from_millis(expiration.deadline - now))
                }
                Some(ref expiration) => {
                    self.process_expiration(expiration);
                    self.set_elapsed(expiration.deadline);
                }
                None => {
                    self.set_elapsed(now);
                    break;
                }
            }
        }

        match self.trigger.pop_back() {
            None => TimeOut::None,
            Some(handle) => TimeOut::ClockEntry(handle),
        }
    }
}

// Level in the wheel.
// All level contains 64 slots.
pub struct Level {
    // current level
    level: usize,

    // Determine which slot contains entries based on occupied bit.
    occupied: u64,

    // slots in a level.
    slots: [LinkedList<Clock>; SLOTS_NUM],
}

impl Level {
    // Specify the level and create a Level structure.
    pub(crate) fn new(level: usize) -> Self {
        let mut slots: [MaybeUninit<LinkedList<Clock>>; SLOTS_NUM] =
            unsafe { MaybeUninit::uninit().assume_init() };

        for slot in slots.iter_mut() {
            *slot = MaybeUninit::new(Default::default());
        }

        unsafe {
            let slots = mem::transmute::<_, [LinkedList<Clock>; SLOTS_NUM]>(slots);
            Self {
                level,
                occupied: 0,
                slots,
            }
        }
    }

    // Based on the elapsed which the current time wheel is running,
    // and the expected expiration time of the clock_entry,
    // find the corresponding slot and insert it.
    pub(crate) fn insert(&mut self, mut clock_entry: NonNull<Clock>) {
        // This duration represents how long it takes for the current slot to complete,
        // at least 0.
        let duration = unsafe { clock_entry.as_ref().expiration() };

        // Unsafe access to clock_entry is only unsafe when Sleep Drop,
        // `Sleep` here does not go into `Ready`.
        unsafe { clock_entry.as_mut().set_duration(duration) };

        let slot = ((duration >> (self.level * LEVELS_NUM)) % SLOTS_NUM as u64) as usize;
        self.slots[slot].push_front(clock_entry);

        self.occupied |= 1 << slot;
    }

    pub(crate) fn cancel(&mut self, clock_entry: NonNull<Clock>) {
        // Unsafe access to clock_entry is only unsafe when Sleep Drop,
        // `Sleep` here does not go into `Ready`.
        let duration = unsafe { clock_entry.as_ref().duration() };

        let slot = ((duration >> (self.level * LEVELS_NUM)) % SLOTS_NUM as u64) as usize;

        // Caller has unique access to the linked list.
        // The clock entry is guaranteed to be inside the wheel, so we need to unset the
        // occupied bit.
        unsafe {
            self.slots[slot].remove(clock_entry);
        }

        if self.slots[slot].is_empty() {
            // Unset the bit
            self.occupied &= !(1 << slot);
        }
    }

    // Return where the next expiration is located, and its deadline.
    pub(crate) fn next_expiration(&self, now: u64) -> Option<Expiration> {
        let slot = self.next_occupied_slot(now)?;

        let deadline = Self::calculate_deadline(slot, self.level, now);

        Some(Expiration {
            level: self.level,
            slot,
            deadline,
        })
    }

    fn calculate_deadline(slot: usize, level: usize, now: u64) -> u64 {
        let slot_range = slot_range(level);
        let level_range = slot_range * SLOTS_NUM as u64;
        let level_start = now & !(level_range - 1);
        // Add the time of the last slot at this level to represent a time period.
        let mut deadline = level_start + slot as u64 * slot_range;

        if deadline <= now {
            // This only happened when Duration > MAX_DURATION
            deadline += level_range;
        }

        deadline
    }

    // Find the next slot that needs to be executed.
    pub(crate) fn next_occupied_slot(&self, now: u64) -> Option<usize> {
        if self.occupied == 0 {
            return None;
        }

        let now_slot = now / slot_range(self.level);
        let occupied = self.occupied.rotate_right(now_slot as u32);
        let zeros = occupied.trailing_zeros();
        let slot = (zeros as u64 + now_slot) % SLOTS_NUM as u64;

        Some(slot as usize)
    }

    // Fetch all timers in a slot of the corresponding level.
    pub(crate) fn take_slot(&mut self, slot: usize) -> LinkedList<Clock> {
        self.occupied &= !(1 << slot);
        mem::take(&mut self.slots[slot])
    }
}

// All the slots before this level add up to approximately.
fn slot_range(level: usize) -> u64 {
    SLOTS_NUM.pow(level as u32) as u64
}

#[cfg(test)]
mod test {
    use crate::time::wheel::{Level, Wheel, LEVELS_NUM};
    cfg_net!(
        #[cfg(feature = "ffrt")]
        use crate::time::TimeDriver;
        use crate::time::{sleep, timeout};
        use crate::net::UdpSocket;
        use crate::task::JoinHandle;
        use std::net::SocketAddr;
        use std::time::Duration;
    );

    /// UT test cases for Wheel::new
    ///
    /// # Brief
    /// 1. Use Wheel::new to create a Wheel Struct.
    /// 2. Verify the data in the Wheel Struct.
    #[test]
    fn ut_wheel_new_test() {
        let wheel = Wheel::new();
        assert_eq!(wheel.elapsed, 0);
        assert_eq!(wheel.levels.len(), LEVELS_NUM);
    }

    /// UT test cases for Sleep drop.
    ///
    /// # Brief
    /// 1. Use timeout to create a Timeout Struct.
    /// 2. Enable the Sleep Struct corresponding to the Timeout Struct to enter
    ///    the Pending state.
    /// 3. Verify the change of the internal TimerHandle during Sleep Struct
    ///    drop.
    #[test]
    #[cfg(feature = "net")]
    fn ut_sleep_drop() {
        const ADDR: &str = "127.0.0.1:0";

        async fn udp_sender(rx: crate::sync::oneshot::Receiver<SocketAddr>) {
            let sender = UdpSocket::bind(ADDR).await.unwrap();
            let buf = [2; 10];
            sleep(Duration::from_secs(1)).await;
            let receiver_addr = rx.await.unwrap();
            sender.send_to(buf.as_slice(), receiver_addr).await.unwrap();
        }

        async fn udp_receiver(tx: crate::sync::oneshot::Sender<SocketAddr>) {
            let receiver = UdpSocket::bind(ADDR).await.unwrap();
            let addr = receiver.local_addr().unwrap();
            tx.send(addr).unwrap();
            let mut buf = [0; 10];
            assert!(
                timeout(Duration::from_secs(2), receiver.recv_from(&mut buf[..]))
                    .await
                    .is_ok()
            );
        }

        let mut tasks: Vec<JoinHandle<()>> = Vec::new();
        let (tx, rx) = crate::sync::oneshot::channel();
        tasks.push(crate::spawn(udp_sender(rx)));
        tasks.push(crate::spawn(udp_receiver(tx)));
        for t in tasks {
            let _ = crate::block_on(t);
        }
        #[cfg(feature = "ffrt")]
        let lock = TimeDriver::get_ref().wheel.lock().unwrap();
        #[cfg(feature = "ffrt")]
        for slot in lock.levels[1].slots.iter() {
            assert!(slot.is_empty());
        }
    }

    /// UT test cases for Level::calculate_deadline
    ///
    /// # Brief
    /// 1. Use Level::calculate_deadline() to calculate Level.
    /// 2. Verify the deadline is right.
    #[test]
    fn ut_wheel_calculate_deadline() {
        let deadline = Level::calculate_deadline(36, 0, 95);
        assert_eq!(deadline, 100);
        let deadline = Level::calculate_deadline(1, 1, 63);
        assert_eq!(deadline, 64);
        let deadline = Level::calculate_deadline(37, 0, 79);
        assert_eq!(deadline, 101);
        let deadline = Level::calculate_deadline(31, 1, 960);
        assert_eq!(deadline, 1984);
        let deadline = Level::calculate_deadline(61, 1, 7001);
        assert_eq!(deadline, 8000);
        let deadline = Level::calculate_deadline(2, 2, 8001);
        assert_eq!(deadline, 8192);
        let deadline = Level::calculate_deadline(12, 1, 8192);
        assert_eq!(deadline, 8960);
        let deadline = Level::calculate_deadline(40, 0, 8960);
        assert_eq!(deadline, 9000);
    }
}

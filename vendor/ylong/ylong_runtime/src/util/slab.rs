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

//! ## `slab` can allocate storage space for the same data type
//!
//! `slab` will pre-allocate space for the stored data.
//! When the amount of stored data exceeds the pre-allocated space,
//! `slab` has a growth strategy similar to the [`vec`][vec] module,
//! and when new space is needed, the `slab` grows to **twice**.
//!
//! ### Page
//!
//! The primary storage space in `slab` is a two-dimensional array
//! that holds ['vec'][vec] containers on each page, which grows as
//! `page` grows, with `page` initially being 32 in length and each
//! new `page` added thereafter requiring a length of 2x. The total
//! number of pages in `page` is 19.
//!
//! ### Release
//!
//! When a piece of data in `slab` is no longer in use and is freed,
//! the space where the current data store is located should be reused,
//! and this operation will be used in conjunction with the allocation
//! operation.
//!
//! ### Allocate
//!
//! There are two cases of space allocation for `slab`. One case is
//! that the current space has never been allocated before, then normal
//! space allocation is done for the current container and the parameters
//! are updated. In the other case, it is used in conjunction with the
//! function release. i.e., when the allocation is done again, the space
//! where the previously freed data is located will be used.
//!
//! ### Compact
//!
//! is used to clean up the resources in the `slab` container after a
//! specific number of loops, which is one of the most important uses
//! of this container, to clean up the space that has been allocated
//! but has not yet been used.
//!
//! [vec]: https://doc.rust-lang.org/std/vec/index.html

use std::cell::UnsafeCell;
use std::ops::Deref;
use std::sync::atomic::Ordering::{Relaxed, SeqCst};
use std::sync::atomic::{AtomicBool, AtomicUsize};
use std::sync::{Arc, Mutex};

/// The maximum number of `pages` that `Slab` can hold
const NUM_PAGES: usize = 19;

/// The minimum number of `slots` that `page` can hold
const PAGE_INITIAL_SIZE: usize = 32;
const PAGE_INDEX_SHIFT: u32 = PAGE_INITIAL_SIZE.trailing_zeros() + 1;

/// trait bounds mechanism, so that the binder must implement the `Entry` and
/// `Default` trait methods
pub trait Entry: Default {
    /// Resets the entry.
    fn reset(&self);
}

/// Reference to data stored in `slab`
pub struct Ref<T> {
    value: *const Value<T>,
}

/// Release operation of data stored in `slab` for reuse in the next allocated
/// space
impl<T> Drop for Ref<T> {
    fn drop(&mut self) {
        unsafe {
            let _ = (*self.value).release();
        }
    }
}

/// Provide unquote operation for user-friendly operation
impl<T> Deref for Ref<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe { &(*self.value).value }
    }
}

unsafe impl<T: Sync> Sync for Ref<T> {}
unsafe impl<T: Sync> Send for Ref<T> {}

/// The Address of the stored data.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Address(usize);

/// Gets the bit size of a pointer.
pub const fn pointer_width() -> u32 {
    std::mem::size_of::<usize>() as u32 * 8
}

impl Address {
    /// Get the number of `page` pages at the current address
    pub fn page(&self) -> usize {
        let slot_shifted = (self.0 + PAGE_INITIAL_SIZE) >> PAGE_INDEX_SHIFT;
        (pointer_width() - slot_shifted.leading_zeros()) as usize
    }

    /// Convert `Address` to `usize`
    pub const fn as_usize(self) -> usize {
        self.0
    }

    /// Convert `usize` to `Address`
    pub fn from_usize(src: usize) -> Address {
        Address(src)
    }
}

/// Amortized allocation for homogeneous data types.
pub struct Slab<T> {
    /// Essentially a two-dimensional array, the constituent units in the
    /// container
    pages: [Arc<Page<T>>; NUM_PAGES],
}

impl<T: Entry> Default for Slab<T> {
    fn default() -> Slab<T> {
        Slab::new()
    }
}

impl<T: Entry> Slab<T> {
    /// Set up the initialization parameters
    pub fn new() -> Slab<T> {
        let mut slab = Slab {
            pages: Default::default(),
        };

        // The minimum number of `slots` that can fit in a `page` at initialization,
        // where the default value is 32
        let mut len = PAGE_INITIAL_SIZE;
        // The sum of the lengths of all `pages` before this `page`, i.e. the sum of
        // `len`
        let mut prev_len: usize = 0;

        for page in &mut slab.pages {
            // we've just initialized the pages, so there is no other `Arc`
            let page = Arc::get_mut(page).unwrap();
            page.len = len;
            page.prev_len = prev_len;
            // The `len` of each `page` will be doubled from the previous one
            len *= 2;
            prev_len += page.len;
        }

        slab
    }

    /// Easy to call for allocation
    pub fn handle(&self) -> Slab<T> {
        Slab {
            pages: self.pages.clone(),
        }
    }

    /// Space allocation for containers
    ///
    /// # Safety
    /// 1. The essence of space allocation to the container is actually to
    ///    allocate each page of the container for the operation
    /// 2. Before allocating each page of the container, we will try to get lock
    ///    permission to prevent multiple threads from having permission to
    ///    modify the state
    ///
    /// Using pointers
    pub unsafe fn allocate(&self) -> Option<(Address, Ref<T>)> {
        // Find the first available `slot`
        for page in &self.pages[..] {
            if let Some((addr, val)) = Page::allocate(page) {
                return Some((addr, val));
            }
        }

        None
    }

    /// Iterating over the data in the container
    pub fn for_each(&mut self, mut f: impl FnMut(&T)) {
        for page_idx in 0..self.pages.len() {
            let slots = self.pages[page_idx].slots.lock().unwrap();

            for slot_idx in 0..slots.slots.len() {
                unsafe {
                    let slot = slots.slots.as_ptr().add(slot_idx);
                    let value = slot.cast::<Value<T>>();

                    f(&(*value).value);
                }
            }
        }
    }

    /// Used to get the reference stored at the given address
    pub fn get(&mut self, addr: Address) -> Option<&T> {
        let page_idx = addr.page();
        let slot_idx = self.pages[page_idx].slot(addr);

        if !self.pages[page_idx].allocated.load(SeqCst) {
            return None;
        }

        unsafe {
            // Fetch by pointer, usage is similar to `C`
            let slot = self.pages[page_idx]
                .slots
                .lock()
                .unwrap()
                .slots
                .as_ptr()
                .add(slot_idx);

            let value = slot.cast::<Value<T>>();

            Some(&(*value).value)
        }
    }

    /// Used to clean up the resources in the `Slab` container after a specific
    /// number of loops, which is one of the most important uses of this
    /// container
    ///
    /// # Safety
    /// Releasing resources here does not release resources that are being used
    /// or have not yet been allocated
    /// 1. The release of each page will initially determine if the resources on
    ///    the current page are being used or if the current page has not been
    ///    allocated
    /// 2. Next, it will determine whether the `slots` of the current page are
    ///    owned by other threads to prevent its resources from changing to the
    ///    used state
    /// 3. Finally, the checks are performed again, with the same checks as in
    ///    the first step, to prevent state changes and ensure that no errors or
    ///    invalid releases are made
    ///
    /// Using atomic variables
    pub unsafe fn compact(&mut self) {
        for page in (self.pages[1..]).iter() {
            // The `slots` of the current `page` are being used, or the current `page` is
            // not allocated and not cleaned up.
            if page.used.load(Relaxed) != 0 || !page.allocated.load(Relaxed) {
                continue;
            }

            // The current `slots` are being owned by other threads and are not cleaned up.
            let mut slots = match page.slots.try_lock() {
                Ok(slots) => slots,
                _ => continue,
            };

            // Check again, if the `slots` of the current `page` are being used, or if the
            // current `page` is not allocated, do not clean up.
            if slots.used > 0 || slots.slots.capacity() == 0 {
                continue;
            }

            page.allocated.store(false, Relaxed);

            let vec = std::mem::take(&mut slots.slots);
            slots.head = 0;

            drop(slots);
            drop(vec);
        }
    }
}

struct Page<T> {
    // Number of `slots` currently being used
    pub used: AtomicUsize,
    // Whether the current `page` is allocated space
    pub allocated: AtomicBool,
    // The number of `slots` that `page` can hold
    pub len: usize,
    // The sum of the lengths of all `pages` before the `page`, i.e. the sum of the number of
    // `slots`
    pub prev_len: usize,
    // `Slots`
    pub slots: Mutex<Slots<T>>,
}

unsafe impl<T: Sync> Sync for Page<T> {}
unsafe impl<T: Sync> Send for Page<T> {}

impl<T> Page<T> {
    // Get the location of the `slot` in the current `page` based on the current
    // `Address`.
    fn slot(&self, addr: Address) -> usize {
        addr.0 - self.prev_len
    }

    // Get the current `Address` based on the `slot` location in the current `page`
    fn addr(&self, slot: usize) -> Address {
        Address(slot + self.prev_len)
    }

    fn release(&self, value: *const Value<T>) {
        let mut locked = self.slots.lock().unwrap();

        // Get the current `slot` based on the `value` value
        let idx = locked.index_for(value);
        locked.slots[idx].next = locked.head as u32;
        locked.head = idx;
        locked.used -= 1;

        self.used.store(locked.used, Relaxed);
    }
}

impl<T: Entry> Page<T> {
    unsafe fn allocate(me: &Arc<Page<T>>) -> Option<(Address, Ref<T>)> {
        if me.used.load(Relaxed) == me.len {
            return None;
        }

        let mut locked = me.slots.lock().unwrap();

        if locked.head < locked.slots.len() {
            let locked = &mut *locked;

            let idx = locked.head;
            let slot = &locked.slots[idx];

            locked.head = slot.next as usize;

            locked.used += 1;
            me.used.store(locked.used, Relaxed);

            (*slot.value.get()).value.reset();

            Some((me.addr(idx), slot.gen_ref(me)))
        } else if me.len == locked.slots.len() {
            None
        } else {
            let idx = locked.slots.len();

            if idx == 0 {
                locked.slots.reserve_exact(me.len);
            }

            locked.slots.push(Slot {
                value: UnsafeCell::new(Value {
                    value: Default::default(),
                    page: &**me as *const _,
                }),
                next: 0,
            });

            locked.head += 1;
            locked.used += 1;
            me.used.store(locked.used, Relaxed);
            me.allocated.store(true, Relaxed);

            Some((me.addr(idx), locked.slots[idx].gen_ref(me)))
        }
    }
}

impl<T> Default for Page<T> {
    fn default() -> Page<T> {
        Page {
            used: AtomicUsize::new(0),
            allocated: AtomicBool::new(false),
            len: 0,
            prev_len: 0,
            slots: Mutex::new(Slots::new()),
        }
    }
}

struct Slots<T> {
    pub slots: Vec<Slot<T>>,
    pub head: usize,
    pub used: usize,
}

impl<T> Slots<T> {
    fn new() -> Slots<T> {
        Slots {
            slots: Vec::new(),
            head: 0,
            used: 0,
        }
    }

    fn index_for(&self, slot: *const Value<T>) -> usize {
        use std::mem;

        // Get the first address of the current `page`
        let base = &self.slots[0] as *const _ as usize;

        // Get the current `slot` address
        let slot = slot as usize;
        // Get `Vec` internal element size
        let width = mem::size_of::<Slot<T>>();

        // Get the current `idx`
        (slot - base) / width
    }
}

#[derive(Debug)]
#[repr(C)]
struct Slot<T> {
    pub value: UnsafeCell<Value<T>>,
    pub next: u32,
}

impl<T> Slot<T> {
    fn gen_ref(&self, page: &Arc<Page<T>>) -> Ref<T> {
        std::mem::forget(page.clone());
        let slot = self as *const Slot<T>;
        let value = slot.cast::<Value<T>>();

        Ref { value }
    }
}

#[derive(Debug)]
#[repr(C)]
struct Value<T> {
    pub value: T,
    pub page: *const Page<T>,
}

impl<T> Value<T> {
    unsafe fn release(&self) -> Arc<Page<T>> {
        let page = Arc::from_raw(self.page);
        page.release(self as *const _);
        page
    }
}

#[cfg(test)]
mod test {
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering::SeqCst;
    use std::sync::{Arc, Mutex};
    use std::thread;

    use super::*;

    struct TestEntry {
        cnt: AtomicUsize,
        id: AtomicUsize,
    }

    impl Default for TestEntry {
        fn default() -> TestEntry {
            TestEntry {
                cnt: AtomicUsize::new(0),
                id: AtomicUsize::new(0),
            }
        }
    }

    impl Entry for TestEntry {
        fn reset(&self) {
            self.cnt.fetch_add(1, SeqCst);
        }
    }
    struct Foo {
        cnt: AtomicUsize,
        id: AtomicUsize,
    }

    impl Default for Foo {
        fn default() -> Foo {
            Foo {
                cnt: AtomicUsize::new(0),
                id: AtomicUsize::new(0),
            }
        }
    }

    impl Entry for Foo {
        fn reset(&self) {
            self.cnt.fetch_add(1, SeqCst);
        }
    }

    /// UT test cases for Slab::new()
    ///
    /// # Brief
    /// 1. Check the parameters for completion of initialization, such as the
    ///    number of pages to be checked, the length of each page.
    #[test]
    fn ut_slab_new() {
        let slab = Slab::<Foo>::new();
        assert_eq!(slab.pages.len(), NUM_PAGES);

        for (index, page) in slab.pages.iter().enumerate() {
            assert_eq!(page.len, PAGE_INITIAL_SIZE * 2_usize.pow(index as u32));
        }
    }

    /// UT test cases for Slab::for_each()
    ///
    /// # Brief
    /// 1. To deposit data into the container, call this function to verify that
    ///    the data is correctly deposited stored and can be matched one by one.
    #[test]
    fn ut_slab_for_each() {
        let mut slab = Slab::<Foo>::new();
        let alloc = slab.handle();

        unsafe {
            // Find the first available `slot` and return its `addr` and `Ref`
            let (_, foo1) = alloc.allocate().unwrap();
            // Modify the current `id`
            foo1.id.store(1, SeqCst);

            // Find the second available `slot` and return its `addr` and `Ref`
            let (_, foo2) = alloc.allocate().unwrap();
            foo2.id.store(2, SeqCst);

            // Find the second available `slot` and return its `addr` and `Ref`
            let (_, foo3) = alloc.allocate().unwrap();
            foo3.id.store(3, SeqCst);
        }

        let mut temp = vec![3, 2, 1];
        slab.for_each(|value| {
            assert_eq!(temp.pop().unwrap(), value.id.load(SeqCst));
        });
    }

    /// UT test cases for Slab::get()
    ///
    /// # Brief
    /// 1. Allocate container space and deposit data, get the data address,and
    ///    see if the data can be fetched.
    /// 2. Create invalid data address to see if data can be obtained.
    #[test]
    fn ut_slab_get() {
        let mut slab = Slab::<Foo>::new();

        unsafe {
            let (addr, _) = slab.allocate().unwrap();
            assert!(slab.get(addr).is_some());

            let un_addr = Address::from_usize(10000);
            assert!(slab.get(un_addr).is_none());
        }
    }

    /// UT test cases for Slab::compact()
    ///
    /// # Brief
    /// 1. Pages with allocated space on the first page are not set to
    ///    unallocated even if they are not used.
    /// 2. Pages other than the first page, once assigned and unused, will be
    ///    set to unassigned status.
    #[test]
    fn ut_slab_compact() {
        let mut slab = Slab::<Foo>::new();
        let mut address = Vec::new();

        unsafe {
            for data in 0..33 {
                let (addr, foo) = slab.allocate().unwrap();
                foo.id.store(data, SeqCst);
                address.push((addr, foo));
            }
            slab.compact();
            assert_eq!(slab.get(address[32].0).unwrap().id.load(SeqCst), 32);
        }

        let mut slab = Slab::<Foo>::new();
        let mut address = Vec::new();

        unsafe {
            assert!(!slab.pages[1].allocated.load(SeqCst));

            for _ in 0..33 {
                let (addr, foo) = slab.allocate().unwrap();
                address.push((addr, foo));
            }
            assert!(slab.pages[1].allocated.load(SeqCst));
            assert_eq!(slab.pages[1].used.load(SeqCst), 1);
            drop(address.pop().unwrap().1);
            assert!(slab.pages[1].allocated.load(SeqCst));
            assert_eq!(slab.pages[1].used.load(SeqCst), 0);
            slab.compact();
            assert!(!slab.pages[1].allocated.load(SeqCst));
        }
    }

    /// UT test cases for Slab
    ///
    /// # Brief
    /// 1. Inserting large amounts of data into a container.
    /// 2. Make changes to these inserted data.
    /// 3. Modified data by address verification.
    /// 4. Multiplexing mechanism after calibration is released.
    #[test]
    fn ut_slab_insert_move() {
        let mut slab = Slab::<TestEntry>::new();
        let alloc = slab.handle();

        unsafe {
            // Find the first available `slot` and return its `addr` and `Ref`
            let (addr1, test_entry1) = alloc.allocate().unwrap();
            // Modify the current `id`
            slab.get(addr1).unwrap().id.store(1, SeqCst);
            // The `reset` function has not been called yet, so `cnt` remains unchanged
            assert_eq!(0, slab.get(addr1).unwrap().cnt.load(SeqCst));

            // Find the second available `slot` and return its `addr` and `Ref`
            let (addr2, test_entry2) = alloc.allocate().unwrap();
            slab.get(addr2).unwrap().id.store(2, SeqCst);
            assert_eq!(0, slab.get(addr2).unwrap().cnt.load(SeqCst));

            // This verifies that the function of finding data based on `addr` is working
            assert_eq!(1, slab.get(addr1).unwrap().id.load(SeqCst));
            assert_eq!(2, slab.get(addr2).unwrap().id.load(SeqCst));

            // Active destruct, the `slot` will be reused
            drop(test_entry1);

            assert_eq!(1, slab.get(addr1).unwrap().id.load(SeqCst));

            // Allocate again, but then the allocated `slot` should use the previously
            // destructured `slot`
            let (addr3, test_entry3) = alloc.allocate().unwrap();
            // Comparison, equal is successful
            assert_eq!(addr3, addr1);
            assert_eq!(1, slab.get(addr3).unwrap().cnt.load(SeqCst));
            slab.get(addr3).unwrap().id.store(3, SeqCst);
            assert_eq!(3, slab.get(addr3).unwrap().id.load(SeqCst));

            drop(test_entry2);
            drop(test_entry3);

            // Cleaned regularly, but the first `page` is never cleaned
            slab.compact();
            assert!(slab.get(addr1).is_some());
            assert!(slab.get(addr2).is_some());
            assert!(slab.get(addr3).is_some());
        }
    }

    /// UT test cases for Slab
    ///
    /// # Brief
    /// 1. Inserting large amounts of data into a container
    /// 2. Verify by address that the data is in the correct location
    #[test]
    fn ut_slab_insert_many() {
        unsafe {
            // Verify that `page` is being allocated properly in the case of a large number
            // of inserts.
            let mut slab = Slab::<TestEntry>::new();
            let alloc = slab.handle();
            let mut entries = vec![];

            for i in 0..10_000 {
                let (addr, val) = alloc.allocate().unwrap();
                val.id.store(i, SeqCst);
                entries.push((addr, val));
            }

            for (i, (addr, v)) in entries.iter().enumerate() {
                assert_eq!(i, v.id.load(SeqCst));
                assert_eq!(i, slab.get(*addr).unwrap().id.load(SeqCst));
            }

            entries.clear();

            for i in 0..10_000 {
                let (addr, val) = alloc.allocate().unwrap();
                val.id.store(10_000 - i, SeqCst);
                entries.push((addr, val));
            }

            for (i, (addr, v)) in entries.iter().enumerate() {
                assert_eq!(10_000 - i, v.id.load(SeqCst));
                assert_eq!(10_000 - i, slab.get(*addr).unwrap().id.load(SeqCst));
            }
        }
    }

    /// UT test cases for Slab
    ///
    /// # Brief
    /// 1. Inserting large amounts of data into a container
    /// 2. Verify by address that the data is in the correct location
    #[test]
    fn ut_slab_insert_drop_reverse() {
        unsafe {
            let mut slab = Slab::<TestEntry>::new();
            let alloc = slab.handle();
            let mut entries = vec![];

            for i in 0..10_000 {
                let (addr, val) = alloc.allocate().unwrap();
                val.id.store(i, SeqCst);
                entries.push((addr, val));
            }

            for _ in 0..10 {
                for _ in 0..1_000 {
                    entries.pop();
                }

                for (i, (addr, v)) in entries.iter().enumerate() {
                    assert_eq!(i, v.id.load(SeqCst));
                    assert_eq!(i, slab.get(*addr).unwrap().id.load(SeqCst));
                }
            }
        }
    }

    /// UT test cases for Slab
    ///
    /// # Brief
    /// 1. Multi-threaded allocation of container space, inserting data into it,
    ///    and verifying that the function is correct
    #[test]
    fn ut_slab_multi_allocate() {
        // Multi-threaded either allocating space, or modifying values.
        // finally comparing the values given by the acquired address to be equal.
        let mut slab = Slab::<TestEntry>::new();
        let thread_one_alloc = slab.handle();
        let thread_two_alloc = slab.handle();
        let thread_three_alloc = slab.handle();

        let capacity = 3001;
        let free_queue = Arc::new(Mutex::new(Vec::with_capacity(capacity)));
        let free_queue_2 = free_queue.clone();
        let free_queue_3 = free_queue.clone();
        let free_queue_4 = free_queue.clone();

        unsafe {
            let thread_one = thread::spawn(move || {
                for i in 0..10_00 {
                    let (addr, test_entry) = thread_one_alloc.allocate().unwrap();
                    test_entry.id.store(i, SeqCst);
                    free_queue.lock().unwrap().push((addr, test_entry, i));
                }
            });

            let thread_two = thread::spawn(move || {
                for i in 10_00..20_00 {
                    let (addr, test_entry) = thread_two_alloc.allocate().unwrap();
                    test_entry.id.store(i, SeqCst);
                    free_queue_2.lock().unwrap().push((addr, test_entry, i));
                }
            });

            let thread_three = thread::spawn(move || {
                for i in 20_00..30_00 {
                    let (addr, test_entry) = thread_three_alloc.allocate().unwrap();
                    test_entry.id.store(i, SeqCst);
                    free_queue_3.lock().unwrap().push((addr, test_entry, i));
                }
            });

            thread_one
                .join()
                .expect("Couldn't join on the associated thread");
            thread_two
                .join()
                .expect("Couldn't join on the associated thread");
            thread_three
                .join()
                .expect("Couldn't join on the associated thread");
        }

        for _ in 0..30_00 {
            let temp = free_queue_4.clone().lock().unwrap().pop().unwrap();
            assert_eq!(slab.get(temp.0).unwrap().id.load(SeqCst), temp.2);
        }
    }

    /// UT test cases for Slab
    ///
    /// # Brief
    /// 1. Multi-threaded allocation of container space, inserting data into it,
    ///    and verifying that the function is correct
    /// 2. Free up some of the data space and check if the data is reused in the
    ///    multi-threaded case
    #[test]
    fn ut_slab_multi_allocate_drop() {
        // allocate space and free the used `slot` in the multi-threaded case.
        // retaining the address of the freed `slot` and allocating it again.
        // the address after reallocation is the same as the address of the previously
        // freed `slot`.
        let slab = Slab::<TestEntry>::new();
        let thread_one_alloc = slab.handle();
        let thread_two_alloc = slab.handle();

        let capacity = 2001;
        let free_queue_one = Arc::new(Mutex::new(Vec::with_capacity(capacity)));
        let free_queue_one_2 = free_queue_one.clone();

        let free_queue_two = Arc::new(Mutex::new(Vec::with_capacity(capacity)));
        let free_queue_two_2 = free_queue_two.clone();

        unsafe {
            let thread_one = thread::spawn(move || {
                for i in 0..1000 {
                    let (addr, test_entry) = thread_one_alloc.allocate().unwrap();
                    test_entry.id.store(i, SeqCst);
                    drop(test_entry);

                    free_queue_one.lock().unwrap().push(addr);
                }
            });

            thread_one
                .join()
                .expect("Couldn't join on the associated thread");

            let thread_two = thread::spawn(move || {
                thread::park();
                for i in 0..1000 {
                    let (addr, test_entry) = thread_two_alloc.allocate().unwrap();
                    test_entry.id.store(i, SeqCst);

                    free_queue_two.lock().unwrap().push(addr);
                }
            });

            thread_two.thread().unpark();
            thread_two
                .join()
                .expect("Couldn't join on the associated thread");

            for _ in 0..1000 {
                assert_eq!(
                    free_queue_one_2.clone().lock().unwrap().pop().unwrap(),
                    free_queue_two_2.lock().unwrap().pop().unwrap()
                );
            }
        }
    }
}

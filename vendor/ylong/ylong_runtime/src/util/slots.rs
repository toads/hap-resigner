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

//! Slots container, similar to [`std::collections::LinkedList`]

use std::error::Error;
use std::fmt::{Debug, Display, Formatter};
use std::{fmt, ops};

// Index tag of empty slot, vector will panic if the new capacity exceeds
// isize::MAX bytes.
const NULL: usize = usize::MAX;

#[derive(Debug, Eq, PartialEq)]
struct Entry<T> {
    data: Option<T>,
    prev: usize,
    next: usize,
}

impl<T> Entry<T> {
    fn new(val: T, prev: usize, next: usize) -> Entry<T> {
        Entry {
            data: Some(val),
            prev,
            next,
        }
    }
}

/// An iterator to traverse through the slots
pub struct SlotsIter<'a, T: 'a> {
    entries: &'a Vec<Entry<T>>,
    len: usize,
    head: usize,
}

/// An entry for the slots.
#[derive(Eq, PartialEq)]
pub struct Slots<T> {
    entries: Vec<Entry<T>>,
    pub len: usize,
    head: usize,
    tail: usize,
    next: usize,
}

/// The index of slot to remove is invalid.
#[derive(Debug, Eq, PartialEq, Clone)]
pub struct SlotsError;

impl Display for SlotsError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "invalid key")
    }
}

impl Error for SlotsError {}

impl<T> Slots<T> {
    /// Construct a new 'Slots' container with initial values of 0.
    ///
    /// # Examples
    ///
    /// ```no run
    /// use ylong_runtime::util::slots::Slots;
    ///
    /// let slots: Slots<i32> = Slots::new();
    /// ```
    pub fn new() -> Slots<T> {
        Slots::with_capacity(0)
    }

    pub fn push_back(&mut self, val: T) -> usize {
        let key = self.next;
        let tail = self.tail;

        self.len += 1;

        if key == self.entries.len() {
            // The next slot is exactly the next position of the array.
            // Insert the data into the end of vector.
            self.next = key + 1;
            self.entries.push(Entry::new(val, tail, NULL));
        } else {
            // The next slot is the recycled slot.
            // Update the index of `next` and then insert the data.
            self.next = self.entries[key].next;
            self.entries[key].prev = tail;
            self.entries[key].next = NULL;
            self.entries[key].data = Some(val);
        }

        match self.entries.get_mut(tail) {
            None => {
                self.head = key;
            }
            Some(entry) => {
                entry.next = key;
            }
        }
        self.tail = key;
        key
    }

    /// Pop item from the container head.
    ///
    /// # Examples
    ///
    /// ```no run
    /// use ylong_runtime::util::slots::Slots;
    ///
    /// let mut slots = Slots::new();
    /// let zero = slots.push_back("zero");
    ///
    /// assert_eq!(slots.pop_front(), Some("zero"));
    /// assert!(!slots.contains(zero));
    /// ```
    pub fn pop_front(&mut self) -> Option<T> {
        let curr = self.head;
        if let Some(entry) = self.entries.get_mut(curr) {
            self.head = entry.next;
            // At the next insertion, update the next insertion position.
            entry.prev = NULL;
            entry.next = self.next;
            let val = entry.data.take();
            match self.entries.get_mut(self.head) {
                None => {
                    self.tail = NULL;
                }
                Some(head) => {
                    head.prev = NULL;
                }
            }
            // Update linked-list information.
            self.len -= 1;
            self.next = curr;
            return val;
        }
        None
    }

    /// Delete an element in container.
    ///
    /// # Examples
    ///
    /// ```no run
    /// use ylong_runtime::util::slots::Slots;
    ///
    /// let mut slots = Slots::new();
    /// let zero = slots.push_back("zero");
    ///
    /// assert_eq!(slots.remove(zero), Ok("zero"));
    /// assert!(!slots.contains(zero));
    /// ```
    pub fn remove(&mut self, key: usize) -> Result<T, SlotsError> {
        let entry = self.entries.get_mut(key).ok_or(SlotsError)?;
        let val = entry.data.take().ok_or(SlotsError)?;
        let prev = entry.prev;
        let next = entry.next;
        // At the next insertion, update the next insertion position
        entry.prev = NULL;
        entry.next = self.next;
        // If this node is the header node, update the header node; otherwise, update
        // the `next` of the previous node.
        match self.entries.get_mut(prev) {
            None => {
                self.head = next;
            }
            Some(slot) => {
                slot.next = next;
            }
        }
        // If this node is the tail node, update the tail node; otherwise, update the
        // `prev` of the next node.
        match self.entries.get_mut(next) {
            None => {
                self.tail = prev;
            }
            Some(slot) => {
                slot.prev = prev;
            }
        }
        // Update linked-list information.
        self.len -= 1;
        self.next = key;
        Ok(val)
    }

    /// Check whether the container is empty.
    ///
    /// # Examples
    ///
    /// ```no run
    /// use ylong_runtime::util::slots::Slots;
    ///
    /// let mut slots = Slots::new();
    /// assert!(slots.is_empty());
    ///
    /// slots.push_back(1);
    /// assert!(!slots.is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Construct a new 'Slots' container with a capacity.
    ///
    /// # Examples
    ///
    /// ```no run
    /// use ylong_runtime::util::slots::Slots;
    ///
    /// let slots: Slots<i32> = Slots::with_capacity(0);
    /// ```
    pub fn with_capacity(capacity: usize) -> Slots<T> {
        Slots {
            entries: Vec::with_capacity(capacity),
            head: NULL,
            tail: NULL,
            next: 0,
            len: 0,
        }
    }
    pub(crate) fn get_by_index(&mut self, key: usize) -> Option<& T> {
        if let Some(entry) = self.entries.get_mut(key) {
            let val = entry.data.as_ref();
            return val;
        }
        None
    }
    pub(crate) fn get_first(&mut self) -> Option<& T> {
        let curr = self.head;
        if let Some(entry) = self.entries.get_mut(curr) {
            let val = entry.data.as_ref();
            return val;
        }
        None
    }
}

impl<T> Default for Slots<T> {
    fn default() -> Slots<T> {
        Slots::new()
    }
}

impl<T> ops::Index<usize> for Slots<T> {
    type Output = T;

    fn index(&self, index: usize) -> &Self::Output {
        match self.entries.get(index) {
            Some(entry) => match entry.data {
                Some(ref val) => val,
                None => panic!("invalid index"),
            },
            None => panic!("invalid index"),
        }
    }
}

impl<T> ops::IndexMut<usize> for Slots<T> {
    fn index_mut(&mut self, index: usize) -> &mut Self::Output {
        match self.entries.get_mut(index) {
            Some(entry) => match entry.data {
                Some(ref mut val) => val,
                None => panic!("invalid index"),
            },
            None => panic!("invalid index"),
        }
    }
}

impl<T> Debug for Slots<T>
where
    T: Debug,
{
    fn fmt(&self, fmt: &mut Formatter<'_>) -> fmt::Result {
        write!(
            fmt,
            "Slab {{ len: {}, head: {}, tail: {}, next: {}, cap: {} }}",
            self.len,
            self.head,
            self.tail,
            self.next,
            self.entries.capacity(),
        )
    }
}

impl<'a, T> Iterator for SlotsIter<'a, T> {
    type Item = (usize, &'a T);

    fn next(&mut self) -> Option<(usize, &'a T)> {
        if self.len == 0 {
            return None;
        }
        let head = self.head;
        if let Some(entry) = self.entries.get(head) {
            self.len -= 1;
            self.head = entry.next;
            if let Some(val) = entry.data.as_ref() {
                return Some((head, val));
            }
        }
        None
    }
}

#[cfg(test)]
mod test {
    use crate::util::slots::Slots;
    impl<T> Slots<T> {
        fn get(&self, key: usize) -> Option<&T> {
            match self.entries.get(key) {
                Some(entry) => match entry.data.as_ref() {
                    Some(val) => Some(val),
                    None => None,
                },
                None => None,
            }
        }
    }
    #[derive(Debug, Eq, PartialEq)]
    struct Data {
        inner: i32,
    }

    impl Data {
        fn new(inner: i32) -> Self {
            Data { inner }
        }
    }

    /// UT test cases for Slots::insert().
    ///
    /// # Brief
    /// 1. The next slot is exactly the next position of the array. Insert the
    ///    data into the vector, increase the length, and calculate the next
    ///    insertion position.
    /// 2. The next slot is the recycled slot. Update the index of `next` and
    ///    then insert the data.
    #[test]
    fn ut_slots_push_back() {
        let mut slots = Slots::new();
        let mut keys = Vec::new();

        for data in 0..100 {
            let key = slots.push_back(data);
            keys.push(key);
        }
        for (data, key) in keys.iter().enumerate() {
            assert_eq!(*slots.get(*key).unwrap(), data);
        }

        let mut slots = Slots::new();
        let mut keys = Vec::new();

        for data in 0..100 {
            let key = slots.push_back(data);
            keys.push(key);
        }
        for index in 0..50 {
            let res = slots.remove(index);
            assert!(res.is_ok());
        }
        for data in 100..150 {
            slots.push_back(data);
        }
        let mut cnt = 149;
        for index in 0..50 {
            assert_eq!(*slots.get(index).unwrap(), cnt);
            cnt -= 1;
        }

        let mut slots = Slots::new();
        let mut keys = Vec::new();

        for data in 0..100 {
            let key = slots.push_back(Data::new(data));
            keys.push(key);
        }
        for index in 0..50 {
            let res = slots.remove(index);
            assert!(res.is_ok());
        }
        for data in 100..150 {
            slots.push_back(Data::new(data));
        }
        let mut cnt = 149;
        for index in 0..50 {
            assert_eq!(*slots.get(index).unwrap(), Data::new(cnt));
            cnt -= 1;
        }
    }

    /// UT test cases for Slots::pop_front()
    ///
    /// # Brief
    /// 1. Pop the slot from the head of container.
    #[test]
    fn ut_slots_pop_front() {
        let mut slots = Slots::new();
        for data in 0..100 {
            slots.push_back(data);
        }
        for index in 0..50 {
            assert_eq!(slots.pop_front(), Some(index));
            assert_eq!(slots.get(index), None);
        }
        assert_eq!(slots.len, 50);

        for data in 100..150 {
            slots.push_back(data);
        }
        for target in 50..150 {
            assert_eq!(slots.pop_front(), Some(target));
        }
        assert_eq!(slots.pop_front(), None);
        assert_eq!(slots.len, 0);
    }

    /// UT test cases for Slots::remove().
    ///
    /// # Brief
    /// 1. Get the invalid data location.
    /// 2. Get the valid data location, and it stores data.
    /// 3. Get the valid data location, and it doesn't store data.
    #[test]
    fn ut_slots_remove() {
        let mut slots = Slots::new();
        let mut keys = Vec::new();

        for data in 0..100 {
            let key = slots.push_back(data);
            keys.push(key);
        }
        assert_eq!(slots.remove(0), Ok(0));
    }

    /// UT test cases for Slots::is_empty().
    ///
    /// # Brief
    /// 1. Verify empty container, the result is true.
    /// 2. Verify non-empty container, the result is false.
    #[test]
    fn ut_slots_is_empty() {
        let mut slots = Slots::new();
        assert!(slots.is_empty());

        slots.push_back(1);
        assert!(!slots.is_empty());
    }

    /// UT test cases for Slots
    ///
    /// # Brief
    /// 1. Push a large amount of data into the initialized container.
    /// 2. Check the correctness of inserted data iteratively.
    #[test]
    fn ut_slots_huge_data_push_back() {
        let mut slots = Slots::new();
        let mut keys = Vec::new();

        for data in 0..10000 {
            let key = slots.push_back(data);
            keys.push(key);
        }

        for (index, key) in keys.iter().enumerate() {
            assert_eq!(slots[*key], index);
        }
    }

    /// UT test cases for Slots
    ///
    /// # Brief
    /// 1. Push a large amount of data into the initialized container.
    /// 2. Remove the first half of the container.
    /// 3. Push new data into the container again.
    /// 4. Check the correctness of data sequence and values.
    #[test]
    fn ut_slots_huge_data_remove() {
        let mut slots = Slots::new();
        let mut keys = Vec::new();

        for data in 0..10000 {
            let key = slots.push_back(data);
            keys.push(key);
        }

        for remove_index in 0..5000 {
            let res = slots.remove(remove_index);
            assert!(res.is_ok());
        }

        for data in 10000..15000 {
            slots.push_back(data);
        }

        let mut cnt = 14999;
        for key in 0..5000 {
            assert_eq!(slots[key], cnt);
            cnt -= 1;
        }
    }

    /// UT test cases for Slots
    ///
    /// # Brief
    /// 1. Push data into the initialized container.
    /// 2. Remove slots that have been popped.
    /// 3. Remove slots at wrong index.
    /// 4. Pop the remaining data.
    #[test]
    fn ut_slots_remove_and_pop() {
        let mut slots = Slots::new();

        for data in 0..100 {
            slots.push_back(data);
        }

        for index in 0..10 {
            slots.pop_front();
            let res = slots.remove(index);
            assert!(res.is_err());
        }

        for remove_index in 100..150 {
            let res = slots.remove(remove_index);
            assert!(res.is_err());
        }

        for remove_index in 10..20 {
            let res = slots.remove(remove_index);
            assert!(res.is_ok());
        }

        for index in 20..100 {
            assert_eq!(slots.pop_front(), Some(index));
        }
        assert!(slots.pop_front().is_none());
    }

    /// UT test cases for Slots
    ///
    /// # Brief
    /// 1. Push a large amount of data into the initialized container.
    /// 2. Find data through key-value pairs.
    #[test]
    fn ut_slots_huge_data_find() {
        let mut slots = Slots::new();
        let mut keys = Vec::new();

        for data in 0..10000 {
            let key = slots.push_back(data);
            keys.push(key);
        }

        for key in keys {
            assert_eq!(slots[key], key);
        }
    }

    /// UT test cases for Slots
    ///
    /// # Brief
    /// 1. Push a large amount of data into the initialized container.
    /// 2. Pop the first half of the container.
    /// 3. Push new data into the container again.
    /// 4. Pop all of the data and check correctness of data sequence and
    ///    values.
    #[test]
    fn ut_slots_huge_data_pop_front() {
        let mut slots = Slots::new();

        for data in 0..10000 {
            slots.push_back(data);
        }

        for _ in 0..5000 {
            slots.pop_front();
        }

        for data in 10000..15000 {
            slots.push_back(data);
        }

        let mut cnt = 14999;
        for key in 0..5000 {
            assert_eq!(slots[key], cnt);
            cnt -= 1;
        }
    }
}

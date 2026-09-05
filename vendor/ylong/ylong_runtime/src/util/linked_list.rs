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

//! This linked list does not have ownership of nodes, and it treats the
//! structure passed in by the user as a node for storage, so the `clear`
//! operation does not release memory, and the `remove` operation needs to
//! ensure that the node is in any linked list held by a caller to ensure the
//! memory validity of pointers within the node. Users need to manage the memory
//! of the instances associated with each node themselves.

#![cfg_attr(feature = "ffrt", allow(unused))]

use std::ptr::NonNull;

#[derive(Default)]
#[repr(C)]
pub(crate) struct Node<T> {
    prev: Option<NonNull<T>>,
    next: Option<NonNull<T>>,
}

impl<T> Node<T> {
    pub(crate) fn new() -> Node<T> {
        Node {
            prev: None,
            next: None,
        }
    }
}

impl<T: Link> Node<T> {
    unsafe fn remove_node(node: NonNull<T>) -> Option<NonNull<T>> {
        let prev = T::node(node).as_ref().prev;
        let next = T::node(node).as_ref().next;
        match prev {
            None => return None,
            Some(prev) => T::node(prev).as_mut().next = next,
        }
        match next {
            None => return None,
            Some(next) => T::node(next).as_mut().prev = prev,
        }
        T::node(node).as_mut().prev = None;
        T::node(node).as_mut().next = None;
        Some(node)
    }
}

unsafe impl<T: Send> Send for Node<T> {}
unsafe impl<T: Sync> Sync for Node<T> {}

pub(crate) struct LinkedList<L: Link + Default> {
    head: NonNull<L>,
}

unsafe impl<L: Link + Default + Send> Send for LinkedList<L> {}
unsafe impl<L: Link + Default + Sync> Sync for LinkedList<L> {}

/// Defines the structure of a linked list node.
/// Provides methods for converting between nodes and instances.
///
/// # Safety
///
/// The implementation must ensure that the inserted data does not move in
/// memory.
pub(crate) unsafe trait Link {
    unsafe fn node(ptr: NonNull<Self>) -> NonNull<Node<Self>>
    where
        Self: Sized;
}

impl<L: Link + Default> LinkedList<L> {
    /// Constructs a new linked list.
    pub(crate) fn new() -> LinkedList<L> {
        let head = Box::<L>::default();
        let head_ptr = unsafe { NonNull::new_unchecked(Box::into_raw(head)) };
        let node = unsafe { L::node(head_ptr).as_mut() };
        node.prev = Some(head_ptr);
        node.next = Some(head_ptr);
        LinkedList { head: head_ptr }
    }

    /// Inserts an element to the front of the list.
    pub(crate) fn push_front(&mut self, val: NonNull<L>) {
        unsafe {
            let head = L::node(self.head).as_mut();
            L::node(val).as_mut().next = head.next;
            L::node(val).as_mut().prev = Some(self.head);

            let node = Some(val);
            if let Some(first) = head.next {
                L::node(first).as_mut().prev = node;
            }
            head.next = node;
        }
    }

    /// Pops an element from the back of the list.
    pub(crate) fn pop_back(&mut self) -> Option<NonNull<L>> {
        unsafe {
            let head = L::node(self.head).as_mut();
            if head.prev != Some(self.head) {
                // the queue is not empty, so prev must be some
                let node = head.prev.take().unwrap();
                Node::remove_node(node);
                Some(node)
            } else {
                None
            }
        }
    }

    /// Deletes an element in list.
    ///
    /// # Safety
    ///
    /// This method can be safely used when the node is in a guarded linked list
    /// that the caller has unique access to or the node is not in any
    /// linked list.
    #[cfg(any(feature = "time", feature = "net"))]
    pub(crate) unsafe fn remove(&mut self, node: NonNull<L>) -> Option<NonNull<L>> {
        Node::remove_node(node)
    }

    /// Checks whether the list is empty.
    #[cfg(feature = "time")]
    pub(crate) fn is_empty(&self) -> bool {
        unsafe { L::node(self.head).as_ref().next == Some(self.head) }
    }

    /// Traverses the list and applies the closure on each element. If the
    /// element meets the condition, removes it from the list.
    #[cfg(feature = "net")]
    pub(crate) fn drain_filtered<F>(&mut self, mut f: F)
    where
        F: FnMut(&mut L) -> bool,
    {
        unsafe {
            let head = L::node(self.head).as_ref();
            let mut p = head.next;
            while p != Some(self.head) {
                // p is not head, therefore it must be some
                let node = p.unwrap();
                let next = L::node(node).as_ref().next;
                if f(&mut *node.as_ptr()) {
                    Node::remove_node(node);
                }
                p = next;
            }
        }
    }
}

impl<L: Link + Default> Default for LinkedList<L> {
    fn default() -> Self {
        LinkedList::new()
    }
}

impl<L: Link + Default> Drop for LinkedList<L> {
    fn drop(&mut self) {
        let _ = unsafe { Box::from_raw(self.head.as_ptr()) };
    }
}

#[cfg(test)]
mod tests {
    use std::ptr::{addr_of_mut, NonNull};

    use crate::util::linked_list::{Link, LinkedList, Node};

    #[derive(Default)]
    struct Entry {
        val: usize,
        node: Node<Entry>,
    }

    impl Entry {
        fn new(val: usize) -> Entry {
            Entry {
                val,
                node: Node::new(),
            }
        }

        fn get_ptr(&self) -> NonNull<Self> {
            NonNull::from(self)
        }
    }

    unsafe fn address_of_node(mut ptr: NonNull<Entry>) -> NonNull<Node<Entry>> {
        let node_ptr = addr_of_mut!(ptr.as_mut().node);
        NonNull::new_unchecked(node_ptr)
    }

    fn get_val(ptr: NonNull<Entry>) -> usize {
        unsafe { ptr.as_ref().val }
    }

    unsafe impl Link for Entry {
        unsafe fn node(ptr: NonNull<Self>) -> NonNull<Node<Self>> {
            address_of_node(ptr)
        }
    }

    /// UT test cases for `is_empty()` and `clear()`.
    ///
    /// # Brief
    /// 1. Create a linked list.
    /// 2. Check if the list is empty before and after pushing nodes into the
    /// list.
    /// 3. Check if the list is empty before and after clear the list.
    #[test]
    #[cfg(feature = "time")]
    fn ut_link_list_is_empty() {
        let mut list = LinkedList::<Entry>::new();
        assert!(list.is_empty());
        let node1 = Entry::new(1);
        let node1 = node1.get_ptr();
        list.push_front(node1);
        assert!(!list.is_empty());
    }

    /// UT test cases for `push_front()` and `pop_back()`.
    ///
    /// # Brief
    /// 1. Create a linked list.
    /// 2. Push nodes into the list.
    /// 3. Pop nodes from the list and check the value.
    #[test]
    fn ut_link_list_push_and_pop() {
        let mut list = LinkedList::<Entry>::new();
        let node1 = Entry::new(1);
        let node1 = node1.get_ptr();
        let node2 = Entry::new(2);
        let node2 = node2.get_ptr();
        let node3 = Entry::new(3);
        let node3 = node3.get_ptr();
        list.push_front(node1);
        list.push_front(node2);
        list.push_front(node3);
        assert_eq!(1, get_val(list.pop_back().unwrap()));
        assert_eq!(2, get_val(list.pop_back().unwrap()));
        assert_eq!(3, get_val(list.pop_back().unwrap()));
        assert!(list.pop_back().is_none());
    }

    /// UT test cases for `push_front()` and `remove()`.
    ///
    /// # Brief
    /// 1. Create a linked list.
    /// 2. Push nodes into the list.
    /// 3. Remove the first node from the list and check the list.
    /// 4. Remove the second node from the list and check the list.
    /// 5. Remove the third node from the list and check the list.
    #[cfg(any(feature = "time", feature = "net"))]
    #[test]
    fn ut_link_list_remove() {
        let mut list = LinkedList::<Entry>::new();
        let node1 = Entry::new(1);
        let node1_ptr = node1.get_ptr();
        let node2 = Entry::new(2);
        let node2_ptr = node2.get_ptr();
        let node3 = Entry::new(3);
        let node3_ptr = node3.get_ptr();
        list.push_front(node1_ptr);
        list.push_front(node2_ptr);
        list.push_front(node3_ptr);
        unsafe {
            assert!(list.remove(node1_ptr).is_some());
            assert!(list.remove(node1_ptr).is_none());
            assert_eq!(Some(node2_ptr), node3.node.next);
            assert_eq!(Some(node3_ptr), node2.node.prev);
            assert!(list.remove(node2_ptr).is_some());
            assert!(list.remove(node2_ptr).is_none());
            assert!(list.remove(node3_ptr).is_some());
            assert!(list.remove(node3_ptr).is_none());
        }

        list.push_front(node1_ptr);
        list.push_front(node2_ptr);
        list.push_front(node3_ptr);
        unsafe {
            assert!(list.remove(node2_ptr).is_some());
            assert!(list.remove(node2_ptr).is_none());
            assert_eq!(Some(node1_ptr), node3.node.next);
            assert_eq!(Some(node3_ptr), node1.node.prev);
            assert!(list.remove(node1_ptr).is_some());
            assert!(list.remove(node1_ptr).is_none());
            assert!(list.remove(node3_ptr).is_some());
            assert!(list.remove(node3_ptr).is_none());
        }

        list.push_front(node1_ptr);
        list.push_front(node2_ptr);
        list.push_front(node3_ptr);
        unsafe {
            assert_eq!(get_val(list.remove(node3_ptr).unwrap()), 3);
            assert!(list.remove(node3_ptr).is_none());
            assert_eq!(Some(node1_ptr), node2.node.next);
            assert_eq!(Some(node2_ptr), node1.node.prev);
            assert_eq!(get_val(list.remove(node1_ptr).unwrap()), 1);
            assert!(list.remove(node1_ptr).is_none());
            assert_eq!(get_val(list.remove(node2_ptr).unwrap()), 2);
            assert!(list.remove(node2_ptr).is_none());
        }
    }

    /// UT test cases for `drain_filtered()`.
    ///
    /// # Brief
    /// 1. Create a linked list.
    /// 2. Push nodes into the list.
    /// 3. Drain filtered the list for node that contains a value of 2.
    #[test]
    #[cfg(all(feature = "net", feature = "time"))]
    fn ut_link_list_for_each_mut() {
        let mut list = LinkedList::<Entry>::new();
        let node1 = Entry::new(1);
        let node1_ptr = node1.get_ptr();
        let node2 = Entry::new(2);
        let node2_ptr = node2.get_ptr();
        let node3 = Entry::new(3);
        let node3_ptr = node3.get_ptr();
        list.push_front(node1_ptr);
        list.push_front(node2_ptr);
        list.push_front(node3_ptr);

        let mut value = 0;
        list.drain_filtered(|x| {
            if x.val == 2 {
                value = x.val;
                return true;
            }
            false
        });
        assert_eq!(value, 2);
        unsafe {
            let node = list.pop_back();
            assert_eq!(node.unwrap().as_mut().val, 1);
            let node = list.pop_back();
            assert_eq!(node.unwrap().as_mut().val, 3);
            let node = list.pop_back();
            assert_eq!(node, None);
        }
    }
}

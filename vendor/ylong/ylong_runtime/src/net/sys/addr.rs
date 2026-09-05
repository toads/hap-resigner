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

use std::future::Future;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::{io, mem, option, vec};
#[cfg(target_os = "linux")]
use libc::{gid_t, uid_t};

use crate::spawn_blocking;
use crate::task::JoinHandle;

pub(crate) async fn each_addr<A: ToSocketAddrs, F, T>(addr: A, mut f: F) -> io::Result<T>
where
    F: FnMut(SocketAddr) -> io::Result<T>,
{
    let addrs = addr.to_socket_addrs().await?;

    let mut last_e = None;

    for addr in addrs {
        match f(addr) {
            Ok(res) => return Ok(res),
            Err(e) => last_e = Some(e),
        }
    }

    Err(last_e.unwrap_or(io::Error::new(
        io::ErrorKind::InvalidInput,
        "addr could not resolve to any address",
    )))
}

#[cfg(target_os = "linux")]
pub(crate) async fn each_addr_with_owner<A: ToSocketAddrs, F, T>(
    uid: uid_t,
    gid: gid_t,
    addr: A,
    mut f: F,
) -> io::Result<T>
where
    F: FnMut(SocketAddr, uid_t, gid_t) -> io::Result<T>,
{
    let addrs = addr.to_socket_addrs().await?;

    let mut last_e = None;

    for addr in addrs {
        match f(addr, uid, gid) {
            Ok(res) => return Ok(res),
            Err(e) => last_e = Some(e),
        }
    }

    Err(last_e.unwrap_or(io::Error::new(
        io::ErrorKind::InvalidInput,
        "addr could not resolve to any address",
    )))
}

/// Convert the type that implements the trait to [`SocketAddr`]
pub trait ToSocketAddrs {
    /// Returned iterator of SocketAddr.
    type Iter: Iterator<Item = SocketAddr>;

    /// Converts this object to an iterator of resolved `SocketAddr`s.
    fn to_socket_addrs(&self) -> State<Self::Iter>;
}

/// Parsing process status, str and (&str, u16) types may be Block
pub enum State<I> {
    Block(JoinHandle<io::Result<I>>),
    Ready(io::Result<I>),
    Done,
}

impl<I: Iterator<Item = SocketAddr>> Future for State<I> {
    type Output = io::Result<I>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        match mem::replace(this, State::Done) {
            State::Block(mut task) => {
                let poll = Pin::new(&mut task).poll(cx)?;
                if poll.is_pending() {
                    *this = State::Block(task);
                }
                poll
            }
            State::Ready(res) => Poll::Ready(res),
            State::Done => unreachable!("cannot poll a completed future"),
        }
    }
}

impl<I> Unpin for State<I> {}

impl ToSocketAddrs for SocketAddr {
    type Iter = option::IntoIter<SocketAddr>;

    fn to_socket_addrs(&self) -> State<Self::Iter> {
        State::Ready(Ok(Some(*self).into_iter()))
    }
}

impl ToSocketAddrs for SocketAddrV4 {
    type Iter = option::IntoIter<SocketAddr>;

    fn to_socket_addrs(&self) -> State<Self::Iter> {
        SocketAddr::V4(*self).to_socket_addrs()
    }
}

impl ToSocketAddrs for SocketAddrV6 {
    type Iter = option::IntoIter<SocketAddr>;

    fn to_socket_addrs(&self) -> State<Self::Iter> {
        SocketAddr::V6(*self).to_socket_addrs()
    }
}

impl ToSocketAddrs for (IpAddr, u16) {
    type Iter = option::IntoIter<SocketAddr>;

    fn to_socket_addrs(&self) -> State<Self::Iter> {
        let (ip, port) = *self;
        match ip {
            IpAddr::V4(ip_type) => (ip_type, port).to_socket_addrs(),
            IpAddr::V6(ip_type) => (ip_type, port).to_socket_addrs(),
        }
    }
}

impl ToSocketAddrs for (Ipv4Addr, u16) {
    type Iter = option::IntoIter<SocketAddr>;

    fn to_socket_addrs(&self) -> State<Self::Iter> {
        let (ip, port) = *self;
        SocketAddrV4::new(ip, port).to_socket_addrs()
    }
}

impl ToSocketAddrs for (Ipv6Addr, u16) {
    type Iter = option::IntoIter<SocketAddr>;

    fn to_socket_addrs(&self) -> State<Self::Iter> {
        let (ip, port) = *self;
        SocketAddrV6::new(ip, port, 0, 0).to_socket_addrs()
    }
}

impl ToSocketAddrs for (&str, u16) {
    type Iter = vec::IntoIter<SocketAddr>;

    fn to_socket_addrs(&self) -> State<Self::Iter> {
        let (host, port) = *self;

        if let Ok(addr) = host.parse::<Ipv4Addr>() {
            let addr = SocketAddrV4::new(addr, port);
            return State::Ready(Ok(vec![SocketAddr::V4(addr)].into_iter()));
        }

        if let Ok(addr) = host.parse::<Ipv6Addr>() {
            let addr = SocketAddrV6::new(addr, port, 0, 0);
            return State::Ready(Ok(vec![SocketAddr::V6(addr)].into_iter()));
        }

        let host = host.to_string();
        let task = spawn_blocking(move || {
            let addr = (host.as_str(), port);
            std::net::ToSocketAddrs::to_socket_addrs(&addr)
        });
        State::Block(task)
    }
}

impl ToSocketAddrs for str {
    type Iter = vec::IntoIter<SocketAddr>;

    fn to_socket_addrs(&self) -> State<Self::Iter> {
        if let Ok(addr) = self.parse() {
            return State::Ready(Ok(vec![addr].into_iter()));
        }

        let addr = self.to_string();
        let task = spawn_blocking(move || {
            let addr = addr.as_str();
            std::net::ToSocketAddrs::to_socket_addrs(addr)
        });
        State::Block(task)
    }
}

impl<'a> ToSocketAddrs for &'a [SocketAddr] {
    type Iter = std::iter::Cloned<std::slice::Iter<'a, SocketAddr>>;

    fn to_socket_addrs(&self) -> State<Self::Iter> {
        State::Ready(Ok(self.iter().cloned()))
    }
}

impl ToSocketAddrs for String {
    type Iter = vec::IntoIter<SocketAddr>;

    fn to_socket_addrs(&self) -> State<Self::Iter> {
        (**self).to_socket_addrs()
    }
}

impl<T: ToSocketAddrs + ?Sized> ToSocketAddrs for &T {
    type Iter = T::Iter;

    fn to_socket_addrs(&self) -> State<Self::Iter> {
        (**self).to_socket_addrs()
    }
}

#[cfg(test)]
mod test {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

    use crate::net::ToSocketAddrs;

    /// UT test cases for `ToSocketAddrs` str.
    ///
    /// # Brief
    /// 1. Create an address with str.
    /// 2. Call `to_socket_addrs()` to convert str to `SocketAddr`.
    /// 3. Check if the test results are correct.
    #[test]
    fn ut_to_socket_addrs_str() {
        let addr_str = "127.0.0.1:8080";
        crate::block_on(async {
            let mut addrs_iter = addr_str.to_socket_addrs().await.unwrap();

            let expected_addr = SocketAddr::from(([127, 0, 0, 1], 8080));
            assert_eq!(Some(expected_addr), addrs_iter.next());
            assert!(addrs_iter.next().is_none());
        });
    }

    /// UT test cases for `ToSocketAddrs` blocking.
    ///
    /// # Brief
    /// 1. Create an address with "localhost".
    /// 2. Call `to_socket_addrs()` to convert str to `SocketAddr`.
    /// 3. Check if the test results are correct.
    #[test]
    fn ut_to_socket_addrs_blocking() {
        let addr_str = "localhost:8080";
        crate::block_on(async {
            let addrs_vec = addr_str
                .to_socket_addrs()
                .await
                .unwrap()
                .collect::<Vec<SocketAddr>>();

            let expected_addr1 = SocketAddr::from(([127, 0, 0, 1], 8080));
            let expected_addr2 = SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], 8080));
            println!("{:?}", addrs_vec);
            assert!(addrs_vec.contains(&expected_addr1) || addrs_vec.contains(&expected_addr2));
        });
    }

    /// UT test cases for `ToSocketAddrs` (&str, u16).
    ///
    /// # Brief
    /// 1. Create an address with (&str, u16).
    /// 2. Call `to_socket_addrs()` to convert str to `SocketAddr`.
    /// 3. Check if the test results are correct.
    #[test]
    fn ut_to_socket_addrs_str_u16() {
        let addr_str = ("127.0.0.1", 8080);
        crate::block_on(async {
            let mut addrs_iter = addr_str.to_socket_addrs().await.unwrap();

            let expected_addr = SocketAddr::from(([127, 0, 0, 1], 8080));
            assert_eq!(Some(expected_addr), addrs_iter.next());
            assert!(addrs_iter.next().is_none());
        });

        let addr_str = ("localhost", 8080);
        crate::block_on(async {
            let addrs_vec = addr_str
                .to_socket_addrs()
                .await
                .unwrap()
                .collect::<Vec<SocketAddr>>();

            let expected_addr1 = SocketAddr::from(([127, 0, 0, 1], 8080));
            let expected_addr2 = SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], 8080));
            assert!(addrs_vec.contains(&expected_addr1) || addrs_vec.contains(&expected_addr2));
        });

        let addr_str = ("::1", 8080);
        crate::block_on(async {
            let mut addrs_iter = addr_str.to_socket_addrs().await.unwrap();

            let expected_addr = SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], 8080));
            assert_eq!(Some(expected_addr), addrs_iter.next());
            assert!(addrs_iter.next().is_none());
        });
    }

    /// UT test cases for `ToSocketAddrs` (ipaddr, u16).
    ///
    /// # Brief
    /// 1. Create an address with (ipaddr, u16).
    /// 2. Call `to_socket_addrs()` to convert str to `SocketAddr`.
    /// 3. Check if the test results are correct.
    #[test]
    fn ut_to_socket_addrs_ipaddr_u16() {
        let addr_str = (IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 8080);
        crate::block_on(async {
            let mut addrs_iter = addr_str.to_socket_addrs().await.unwrap();

            let expected_addr = SocketAddr::from(([127, 0, 0, 1], 8080));
            assert_eq!(Some(expected_addr), addrs_iter.next());
            assert!(addrs_iter.next().is_none());
        });

        let addr_str = (IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)), 8080);
        crate::block_on(async {
            let mut addrs_iter = addr_str.to_socket_addrs().await.unwrap();

            let expected_addr = SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], 8080));
            assert_eq!(Some(expected_addr), addrs_iter.next());
            assert!(addrs_iter.next().is_none());
        });
    }

    /// UT test cases for `ToSocketAddrs` (ipv4addr, u16).
    ///
    /// # Brief
    /// 1. Create an address with (ipv4addr, u16).
    /// 2. Call `to_socket_addrs()` to convert str to `SocketAddr`.
    /// 3. Check if the test results are correct.
    #[test]
    fn ut_to_socket_addrs_ipv4addr_u16() {
        let addr_str = (Ipv4Addr::new(127, 0, 0, 1), 8080);
        crate::block_on(async {
            let mut addrs_iter = addr_str.to_socket_addrs().await.unwrap();

            let expected_addr = SocketAddr::from(([127, 0, 0, 1], 8080));
            assert_eq!(Some(expected_addr), addrs_iter.next());
            assert!(addrs_iter.next().is_none());
        });
    }

    /// UT test cases for `ToSocketAddrs` (ipv6addr, u16).
    ///
    /// # Brief
    /// 1. Create an address with (ipv6addr, u16).
    /// 2. Call `to_socket_addrs()` to convert str to `SocketAddr`.
    /// 3. Check if the test results are correct.
    #[test]
    fn ut_to_socket_addrs_ipv6addr_u16() {
        let addr_str = (Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1), 8080);
        crate::block_on(async {
            let mut addrs_iter = addr_str.to_socket_addrs().await.unwrap();

            let expected_addr = SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], 8080));
            assert_eq!(Some(expected_addr), addrs_iter.next());
            assert!(addrs_iter.next().is_none());
        });
    }
}

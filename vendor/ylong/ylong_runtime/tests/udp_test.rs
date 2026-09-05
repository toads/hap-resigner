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

#![cfg(feature = "net")]

use std::{io, thread};

use ylong_runtime::net::UdpSocket;

const ADDR: &str = "127.0.0.1:0";

/// SDV test cases for `send()` and `recv()`.
///
/// # Brief
/// 1. Create UdpSocket and connect to the remote address.
/// 2. Sender sends message first.
/// 3. Receiver receives message.
/// 4. Check if the test results are correct.
#[test]
fn sdv_udp_send_recv() {
    let handle = ylong_runtime::spawn(async {
        let sender = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
        let receiver = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
        let sender_addr = sender.local_addr().unwrap();
        let receiver_addr = receiver.local_addr().unwrap();

        let connected_sender = sender
            .connect(receiver_addr)
            .await
            .expect("Connect Socket Failed");
        let connected_receiver = receiver
            .connect(sender_addr)
            .await
            .expect("Connect Socket Failed");

        let n = connected_sender
            .send(b"Hello")
            .await
            .expect("Sender Send Failed");
        assert_eq!(n, "Hello".len());

        let mut recv_buf = [0_u8; 12];
        let len = connected_receiver.recv(&mut recv_buf[..]).await.unwrap();

        assert_eq!(&recv_buf[..len], b"Hello");
    });
    ylong_runtime::block_on(handle).expect("block_on failed");
}

/// SDV test cases for `send_to()` and `recv_from()`.
///
/// # Brief
/// 1. Create UdpSocket.
/// 2. Sender sends message to the specified address.
/// 3. Receiver receives message and return the address the message from.
/// 4. Check if the test results are correct.
#[test]
fn sdv_udp_send_to_recv_from() {
    let handle = ylong_runtime::spawn(async {
        let sender = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
        let receiver = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
        let sender_addr = sender.local_addr().unwrap();
        let receiver_addr = receiver.local_addr().unwrap();

        let n = sender
            .send_to(b"Hello", receiver_addr)
            .await
            .expect("Sender Send Failed");
        assert_eq!(n, "Hello".len());

        let mut recv_buf = [0_u8; 12];
        let (len, addr) = receiver.recv_from(&mut recv_buf[..]).await.unwrap();
        assert_eq!(&recv_buf[..len], b"Hello");
        assert_eq!(addr, sender_addr);
    });
    ylong_runtime::block_on(handle).expect("block_on failed");
}

/// SDV test cases for functions in a multithreaded environment.
///
/// # Brief
/// 1. Create sender and receiver threads, bind their new UdpSockets and connect
///    to each other.
/// 2. Sender send message in sender thread.
/// 3. Receiver receives message in receiver thread.
/// 4. Check if the test results are correct.
#[test]
fn sdv_udp_recv() {
    let handle = ylong_runtime::spawn(async move {
        let receiver = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
        let sender = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
        let receiver_addr = receiver.local_addr().unwrap();
        let sender_addr = sender.local_addr().unwrap();
        let connected_receiver = receiver
            .connect(sender_addr)
            .await
            .expect("Connect Socket Failed");
        let connected_sender = sender
            .connect(receiver_addr)
            .await
            .expect("Connect Socket Failed");

        let handle = thread::spawn(move || {
            let handle = ylong_runtime::spawn(async move {
                let n = connected_sender
                    .send(b"Hello")
                    .await
                    .expect("Sender Send Failed");
                assert_eq!(n, "Hello".len());
            });
            ylong_runtime::block_on(handle).expect("block_on failed");
        });

        let mut recv_buf = [0_u8; 12];
        let len = connected_receiver.recv(&mut recv_buf[..]).await.unwrap();
        assert_eq!(&recv_buf[..len], b"Hello");

        handle.join().unwrap();
    });
    ylong_runtime::block_on(handle).expect("block_on failed");
}

/// SDV test cases for `try_send_to()` and `try_recv_from()`.
///
/// # Brief
/// 1. Create UdpSocket.
/// 2. Sender tries to send message to the specified address.
/// 3. Receiver tries to receive message and return the address the message
///    from.
/// 4. Check if the test results are correct.
#[test]
fn sdv_udp_try_recv_from() {
    let handle = ylong_runtime::spawn(async move {
        let sender = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
        let sender_addr = sender.local_addr().unwrap();

        let receiver = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
        let receiver_addr = receiver.local_addr().unwrap();

        sender.writable().await.unwrap();
        let mut ret = sender.try_send_to(b"Hello", receiver_addr);
        while let Err(ref e) = ret {
            if e.kind() == io::ErrorKind::WouldBlock {
                ret = sender.try_send_to(b"Hello", receiver_addr);
            } else {
                panic!("try_send_to failed: {}", e);
            }
        }

        assert_eq!(ret.unwrap(), 5);

        let mut recv_buf = [0_u8; 12];
        receiver.readable().await.unwrap();
        let mut ret = receiver.try_recv_from(&mut recv_buf[..]);
        while let Err(ref e) = ret {
            if e.kind() == io::ErrorKind::WouldBlock {
                ret = receiver.try_recv_from(&mut recv_buf[..]);
            } else {
                panic!("try_send_to failed: {}", e);
            }
        }
        let (len, peer_addr) = ret.unwrap();
        assert_eq!(&recv_buf[..len], b"Hello");
        assert_eq!(peer_addr, sender_addr);
    });
    ylong_runtime::block_on(handle).expect("block_on failed");
}

fn sdv_udp_try_send(connected_sender: ylong_runtime::net::ConnectedUdpSocket) {
    let handle = ylong_runtime::spawn(async move {
        connected_sender.writable().await.unwrap();
        match connected_sender.try_send(b"Hello") {
            Ok(n) => assert_eq!(n, "Hello".len()),
            Err(e) => panic!("Sender Send Failed {e}"),
        }
    });
    ylong_runtime::block_on(handle).expect("block_on failed");
}

/// SDV test cases for try_send and try_recv
///
/// # Brief
/// 1. Create sender and receiver threads, bind their new UdpSockets and connect
///    to each other.
/// 2. Sender waits for writable events and attempts to send message in sender
///    thread.
/// 3. Receiver waits for readable events and attempts to receive message in
///    receiver thread.
/// 4. Check if the test results are correct.
#[test]
fn sdv_udp_try_recv() {
    let handle = ylong_runtime::spawn(async move {
        let receiver = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
        let sender = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
        let receiver_addr = receiver.local_addr().unwrap();
        let sender_addr = sender.local_addr().unwrap();
        let connected_receiver = receiver
            .connect(sender_addr)
            .await
            .expect("Connect Socket Failed");
        let connected_sender = sender
            .connect(receiver_addr)
            .await
            .expect("Connect Socket Failed");

        thread::spawn(move || sdv_udp_try_send(connected_sender));

        connected_receiver.readable().await.unwrap();
        let mut recv_buf = [0_u8; 12];
        let len = connected_receiver.try_recv(&mut recv_buf[..]).unwrap();

        assert_eq!(&recv_buf[..len], b"Hello");
    });
    ylong_runtime::block_on(handle).expect("block_on failed");
}

/// SDV test cases for blocking on try_send and try_recv
///
/// # Brief
/// 1. Create sender and receiver threads, bind their new UdpSockets and connect
///    to each other.
/// 2. Sender waits for writable events and attempts to send message in sender
///    thread.
/// 3. Receiver waits for readable events and attempts to receive message in
///    receiver thread. Calls block_on directly ion it.
/// 4. Check if the test results are correct.
#[test]
#[cfg(not(feature = "ffrt"))]
fn sdv_udp_block_on_try_recv() {
    ylong_runtime::block_on(async {
        let receiver = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
        let receiver_addr = receiver.local_addr().unwrap();
        let sender = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
        let sender_addr = sender.local_addr().unwrap();
        let connected_receiver = receiver
            .connect(sender_addr)
            .await
            .expect("Connect Socket Failed.");
        let connected_sender = sender
            .connect(receiver_addr)
            .await
            .expect("Connect Socket Failed.");

        thread::spawn(move || sdv_udp_try_send(connected_sender));

        connected_receiver.readable().await.unwrap();
        let mut recv_buf = [0_u8; 12];
        let len = connected_receiver.try_recv(&mut recv_buf[..]).unwrap();

        assert_eq!(&recv_buf[..len], b"Hello");
    });
}

/// SDV test cases for `poll_send()` and `poll_recv()`.
///
/// # Brief
/// 1. Create UdpSocket and connect to the remote address.
/// 2. Sender calls poll_fn() to send message first.
/// 3. Receiver calls poll_fn() to receive message.
/// 4. Check if the test results are correct.
#[test]
fn sdv_udp_send_recv_poll() {
    let handle = ylong_runtime::spawn(async move {
        let sender = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
        let receiver = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
        let sender_addr = sender.local_addr().unwrap();
        let receiver_addr = receiver.local_addr().unwrap();

        let connected_sender = sender
            .connect(receiver_addr)
            .await
            .expect("Connect Socket Failed");
        let connected_receiver = receiver
            .connect(sender_addr)
            .await
            .expect("Connect Socket Failed");

        let n = ylong_runtime::futures::poll_fn(|cx| connected_sender.poll_send(cx, b"Hello"))
            .await
            .expect("Sender Send Failed");
        assert_eq!(n, "Hello".len());

        let mut recv_buf = [0_u8; 12];
        let mut read = ylong_runtime::io::ReadBuf::new(&mut recv_buf);
        ylong_runtime::futures::poll_fn(|cx| connected_receiver.poll_recv(cx, &mut read))
            .await
            .unwrap();

        assert_eq!(read.filled(), b"Hello");
    });
    ylong_runtime::block_on(handle).expect("block_on failed");
}

/// SDV test cases for `send_to()` and `peek_from()`.
///
/// # Brief
/// 1. Create UdpSocket.
/// 2. Sender calls send_to() to send message to the specified address.
/// 3. Receiver calls peek_from() to receive message and return the number of
///    bytes peeked.
/// 4. Check if the test results are correct.
#[test]
fn sdv_send_to_peek_from() {
    let handle = ylong_runtime::spawn(async move {
        let sender = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
        let receiver = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
        let receiver_addr = receiver.local_addr().unwrap();

        let buf = [2; 6];
        sender
            .send_to(&buf, receiver_addr)
            .await
            .expect("Send data Failed");

        let mut buf = [0; 10];
        let (number_of_bytes, _) = receiver
            .peek_from(&mut buf)
            .await
            .expect("Didn't receive data");

        assert_eq!(number_of_bytes, 6);
    });

    ylong_runtime::block_on(handle).expect("block_on failed!");
}

/// SDV test cases for `send_to()` and `try_peek_from()`.
///
/// # Brief
/// 1. Create UdpSocket.
/// 2. Sender calls send_to() to send message to the specified address.
/// 3. Receiver calls readable() to wait for the socket to become readable.
/// 4. Receiver calls try_peek_from() to receive message and return the number
///    of bytes peeked.
/// 5. Check if the test results are correct.
#[test]
fn sdv_send_to_try_peek_from() {
    let handle = ylong_runtime::spawn(async {
        let sender = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
        let receiver = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
        let receiver_addr = receiver.local_addr().unwrap();

        let buf = [2; 6];
        let number_of_bytes = sender
            .send_to(&buf, receiver_addr)
            .await
            .expect("Send data Failed");
        assert_eq!(number_of_bytes, 6);

        let mut buf = [0; 10];
        receiver.readable().await.expect("Receiver isn't readable");
        let (number_of_bytes, _) = receiver
            .try_peek_from(&mut buf)
            .expect("Didn't receive data");
        assert_eq!(number_of_bytes, 6);
    });

    ylong_runtime::block_on(handle).expect("block_on failed!");
}

/// SDV test cases for `poll_send_to()` and `poll_recv_from()`.
///
/// # Brief
/// 1. Create UdpSocket.
/// 2. Sender calls poll_fn() to send message to the specified address.
/// 3. Receiver calls poll_fn() to receive message and return the address the
///    message from.
/// 4. Check if the test results are correct.
#[test]
fn sdv_udp_send_to_recv_from_poll() {
    let handle = ylong_runtime::spawn(async move {
        let sender = UdpSocket::bind(ADDR)
            .await
            .expect("Sender Bind Socket Failed");
        let receiver = UdpSocket::bind(ADDR)
            .await
            .expect("Receiver Bind Socket Failed");
        let sender_addr = sender.local_addr().unwrap();
        let receiver_addr = receiver.local_addr().unwrap();

        let n =
            ylong_runtime::futures::poll_fn(|cx| sender.poll_send_to(cx, b"Hello", receiver_addr))
                .await
                .expect("Sender Send Failed");
        assert_eq!(n, "Hello".len());

        let mut recv_buf = [0_u8; 12];
        let mut read = ylong_runtime::io::ReadBuf::new(&mut recv_buf);
        let addr = ylong_runtime::futures::poll_fn(|cx| receiver.poll_recv_from(cx, &mut read))
            .await
            .unwrap();
        assert_eq!(read.filled(), b"Hello");
        assert_eq!(addr, sender_addr);
    });
    ylong_runtime::block_on(handle).expect("block_on failed");
}

/// SDV test cases for `poll_send_to()` and `poll_peek_from()`.
///
/// # Brief
/// 1. Create UdpSocket.
/// 2. Sender calls poll_fn() to send message to the specified address.
/// 3. Receiver calls poll_fn() to receive message and return the address the
///    message from.
/// 4. Check if the test results are correct.
#[test]
fn sdv_udp_send_to_peek_from_poll() {
    let handle = ylong_runtime::spawn(async move {
        let sender = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
        let receiver = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
        let sender_addr = sender.local_addr().unwrap();
        let receiver_addr = receiver.local_addr().unwrap();

        let n =
            ylong_runtime::futures::poll_fn(|cx| sender.poll_send_to(cx, b"Hello", receiver_addr))
                .await
                .expect("Sender Send Failed");
        assert_eq!(n, "Hello".len());

        let mut recv_buf = [0_u8; 12];
        let mut read = ylong_runtime::io::ReadBuf::new(&mut recv_buf);
        let addr = ylong_runtime::futures::poll_fn(|cx| receiver.poll_peek_from(cx, &mut read))
            .await
            .unwrap();
        assert_eq!(read.filled(), b"Hello");
        assert_eq!(addr, sender_addr);
    });
    ylong_runtime::block_on(handle).expect("block_on failed");
}

/// SDV test cases for `broadcast()` and `set_broadcast()`.
///
/// # Brief
/// 1. Create UdpSocket.
/// 2. Sender calls set_broadcast() to set broadcast.
/// 3. Sender calls broadcast() to get broadcast.
/// 4. Check if the test results are correct.
#[test]
fn sdv_set_get_broadcast() {
    let handle = ylong_runtime::spawn(async move {
        let broadcast_socket = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
        broadcast_socket
            .set_broadcast(true)
            .expect("set_broadcast failed");

        assert!(broadcast_socket.broadcast().expect("get broadcast failed"));
    });
    ylong_runtime::block_on(handle).expect("block_on failed");

    let handle = ylong_runtime::spawn(async move {
        let sender = UdpSocket::bind(ADDR).await.unwrap();
        let receiver = UdpSocket::bind(ADDR).await.unwrap();
        let receiver_addr = receiver.local_addr().unwrap();

        let broadcast_socket = sender.connect(receiver_addr).await.unwrap();
        broadcast_socket
            .set_broadcast(true)
            .expect("set_broadcast failed");

        assert!(broadcast_socket.broadcast().expect("get broadcast failed"));
    });
    ylong_runtime::block_on(handle).expect("block_on failed");
}

/// SDV basic test cases for `UdpSocket` with `SocketAddrV4`.
///
/// # Brief
/// 1. Bind and connect `UdpSocket`.
/// 2. Call set_ttl(), ttl(), take_error(), set_multicast_loop_v4(),
///    multicast_loop_v4(), set_multicast_ttl_v4(), multicast_ttl_v4() for
///    `UdpSocket` and `ConnectedUdpSocket`.
/// 3. Check result is correct.
#[test]
fn sdv_udp_basic_v4() {
    ylong_runtime::block_on(async {
        let sender = UdpSocket::bind(ADDR).await.unwrap();
        let receiver = UdpSocket::bind(ADDR).await.unwrap();
        let sender_addr = sender.local_addr().unwrap();
        let receiver_addr = receiver.local_addr().unwrap();

        sender.set_ttl(80).unwrap();
        assert_eq!(sender.ttl().unwrap(), 80);
        assert!(sender.take_error().unwrap().is_none());
        sender.set_multicast_loop_v4(false).unwrap();
        assert!(!sender.multicast_loop_v4().unwrap());
        sender.set_multicast_ttl_v4(42).unwrap();
        assert_eq!(sender.multicast_ttl_v4().unwrap(), 42);

        let interface = std::net::Ipv4Addr::new(0, 0, 0, 0);
        let mut multi_addr = None;

        for i in 0..255 {
            let addr = std::net::Ipv4Addr::new(224, 0, 0, i);
            if sender.join_multicast_v4(&addr, &interface).is_ok() {
                multi_addr = Some(addr);
                break;
            }
        }

        if let Some(addr) = multi_addr {
            sender
                .leave_multicast_v4(&addr, &interface)
                .expect("Cannot leave the multicast group!");
        }

        let connected_sender = sender.connect(receiver_addr).await.unwrap();
        let _connected_receiver = receiver.connect(sender_addr).await.unwrap();

        connected_sender.set_ttl(80).unwrap();
        assert_eq!(connected_sender.ttl().unwrap(), 80);
        assert!(connected_sender.take_error().unwrap().is_none());
        connected_sender.set_multicast_loop_v4(false).unwrap();
        assert!(!connected_sender.multicast_loop_v4().unwrap());
        connected_sender.set_multicast_ttl_v4(42).unwrap();
        assert_eq!(connected_sender.multicast_ttl_v4().unwrap(), 42);

        if let Some(addr) = multi_addr {
            connected_sender
                .join_multicast_v4(&addr, &interface)
                .expect("Cannot join the multicast group!");
            connected_sender
                .leave_multicast_v4(&multi_addr.unwrap(), &interface)
                .expect("Cannot leave the multicast group!");
        }
    });
}

/// SDV basic test cases for `UdpSocket` with `SocketAddrV6`.
///
/// # Brief
/// 1. Bind and connect `UdpSocket`.
/// 2. Call set_multicast_loop_v6(), multicast_loop_v6() for `UdpSocket` and
///    `ConnectedUdpSocket`.
/// 3. Check result is correct.
#[test]
fn sdv_udp_basic_v6() {
    let addr = "::1:0";
    ylong_runtime::block_on(async {
        let sender = UdpSocket::bind(addr).await.unwrap();
        let receiver = UdpSocket::bind(addr).await.unwrap();
        let sender_addr = sender.local_addr().unwrap();
        let receiver_addr = receiver.local_addr().unwrap();

        sender.set_multicast_loop_v6(false).unwrap();
        assert!(!sender.multicast_loop_v6().unwrap());

        let interface = 1_u32;
        let mut multi_addr = None;

        for i in 0..0xFFFF {
            let addr = std::net::Ipv6Addr::new(0xFF02, 0, 0, 0, 0, 0, 0, i);
            if sender.join_multicast_v6(&addr, interface).is_ok() {
                multi_addr = Some(addr);
                break;
            }
        }

        if let Some(addr) = multi_addr {
            sender
                .leave_multicast_v6(&addr, interface)
                .expect("Cannot leave the multicast group!");
        }

        let connected_sender = sender.connect(receiver_addr).await.unwrap();
        let _connected_receiver = receiver.connect(sender_addr).await.unwrap();

        connected_sender.set_multicast_loop_v6(false).unwrap();
        assert!(!connected_sender.multicast_loop_v6().unwrap());

        if let Some(addr) = multi_addr {
            connected_sender
                .join_multicast_v6(&addr, interface)
                .expect("Cannot join the multicast group!");
            connected_sender
                .leave_multicast_v6(&addr, interface)
                .expect("Cannot leave the multicast group!");
        }
    });
}

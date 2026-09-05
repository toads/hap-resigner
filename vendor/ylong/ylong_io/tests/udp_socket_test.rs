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

use std::io::ErrorKind;

use ylong_io::UdpSocket;

/// SDV test cases for `send()` and `recv()`.
///
/// # Brief
/// 1. Create UdpSocket and connect to the remote address.
/// 2. Sender sends message first.
/// 3. Receiver receives message.
/// 4. Check if the test results are correct.
#[test]
fn sdv_send_recv() {
    let sender_addr = "127.0.0.1:8081".parse().unwrap();
    let receiver_addr = "127.0.0.1:8082".parse().unwrap();

    let sender = match UdpSocket::bind(sender_addr) {
        Ok(socket) => socket,
        Err(e) => {
            panic!("Bind Socket Failed {}", e);
        }
    };

    let receiver = match UdpSocket::bind(receiver_addr) {
        Ok(socket) => socket,
        Err(e) => {
            panic!("Bind Socket Failed {}", e);
        }
    };

    let connected_sender = match sender.connect(receiver_addr) {
        Ok(socket) => socket,
        Err(e) => {
            panic!("Connect Socket Failed {}", e);
        }
    };
    let connected_receiver = match receiver.connect(sender_addr) {
        Ok(socket) => socket,
        Err(e) => {
            panic!("Connect Socket Failed {}", e);
        }
    };

    match connected_sender.send(b"Hello") {
        Ok(n) => {
            assert_eq!(n, "Hello".len());
        }
        Err(e) => {
            panic!("Sender Send Failed {}", e);
        }
    }

    let mut recv_buf = [0_u8; 12];
    let len = loop {
        match connected_receiver.recv(&mut recv_buf[..]) {
            Ok(n) => break n,
            Err(e) if e.kind() == ErrorKind::WouldBlock => {}
            Err(e) => panic!("{:?}", e),
        }
    };

    assert_eq!(&recv_buf[..len], b"Hello");
}

/// SDV test cases for `send_to()` and `recv_from()`.
///
/// # Brief
/// 1. Create UdpSocket.
/// 2. Sender sends message to the specified address.
/// 3. Receiver receives message and return the address the message from.
/// 4. Check if the test results are correct.
#[test]
fn sdv_send_to_recv_from() {
    let sender_addr = "127.0.0.1:8085".parse().unwrap();
    let receiver_addr = "127.0.0.1:8086".parse().unwrap();

    let sender = match UdpSocket::bind(sender_addr) {
        Ok(socket) => socket,
        Err(e) => {
            panic!("Bind Socket Failed {}", e);
        }
    };

    let receiver = match UdpSocket::bind(receiver_addr) {
        Ok(socket) => socket,
        Err(e) => {
            panic!("Bind Socket Failed {}", e);
        }
    };

    match sender.send_to(b"Hello", receiver_addr) {
        Ok(n) => {
            assert_eq!(n, "Hello".len());
        }
        Err(e) => {
            panic!("Sender Send Failed {}", e);
        }
    }

    let mut recv_buf = [0_u8; 12];
    let (len, addr) = loop {
        match receiver.recv_from(&mut recv_buf[..]) {
            Ok(res) => break res,
            Err(e) if e.kind() == ErrorKind::WouldBlock => {}
            Err(e) => panic!("{:?}", e),
        }
    };
    assert_eq!(&recv_buf[..len], b"Hello");
    assert_eq!(addr, sender_addr);
}

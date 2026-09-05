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

use std::io::{IoSlice, IoSliceMut, Read, Write};
use std::net::SocketAddr;
use std::{io, net, thread};

use ylong_io::TcpListener;

/// SDV for TcpStream read and write
///
/// # Brief
/// 1. Create a Tcp server
/// 2. Write `hello` to client
/// 3. Read `hello` from client
#[test]
fn sdv_tcp_server() {
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let server = TcpListener::bind(addr).unwrap();
    let addr = server.local_addr().unwrap();

    let thread = thread::spawn(move || {
        let (mut stream, _) = loop {
            let stream = server.accept();
            match stream {
                Ok(stream) => break stream,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(e) => panic!("tcp accept failed: {e:?}"),
            }
        };
        let mut ret = stream.write(b"hello");
        loop {
            match &ret {
                Ok(n) => {
                    assert_eq!(*n, 5);
                    break;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    ret = stream.write(b"hello");
                }
                Err(e) => panic!("tcp write failed: {e:?}"),
            }
        }

        let mut read_stream = stream.try_clone().unwrap();

        let mut buf = [0; 5];
        loop {
            let ret = read_stream.read(&mut buf);
            match &ret {
                Ok(n) => {
                    assert_eq!(*n, 5);
                    assert_eq!(&buf, b"hello");
                    break;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(e) => panic!("tcp write failed: {e:?}"),
            }
        }
    });

    let mut client = loop {
        let tcp = net::TcpStream::connect(addr);
        match tcp {
            Err(_) => continue,
            Ok(stream) => break stream,
        }
    };
    let mut buf = [0; 5];
    let ret = client.read(&mut buf).unwrap();
    assert_eq!(ret, 5);
    assert_eq!(&buf, b"hello");

    let ret = client.write(&buf).unwrap();
    assert_eq!(ret, 5);

    thread.join().unwrap();
}

/// SDV for TcpStream read_vectored and write_vectored
///
/// # Brief
/// 1. Create a Tcp server
/// 2. Write `hello` to client
/// 3. Read `hello` from client
#[test]
fn sdv_tcp_server_vectored() {
    let addr: SocketAddr = "127.0.0.1:0".parse().unwrap();
    let server = TcpListener::bind(addr).unwrap();
    let addr = server.local_addr().unwrap();

    let thread = thread::spawn(move || {
        let (mut stream, _) = loop {
            let stream = server.accept();
            match stream {
                Ok(stream) => break stream,
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(e) => panic!("tcp accept failed: {e:?}"),
            }
        };
        let vec = b"hello";
        let slice = IoSlice::new(vec);
        let mut ret = stream.write_vectored(&[slice]);
        loop {
            match &ret {
                Ok(n) => {
                    assert_eq!(*n, 5);
                    break;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                    ret = stream.write(b"hello");
                    stream.flush().unwrap();
                }
                Err(e) => panic!("tcp write failed: {e:?}"),
            }
        }

        let mut read_stream = stream.try_clone().unwrap();

        loop {
            let mut buf = [0; 5];
            let slice = IoSliceMut::new(&mut buf);
            let ret = read_stream.read_vectored(&mut [slice]);
            match &ret {
                Ok(n) => {
                    assert_eq!(*n, 5);
                    assert_eq!(&buf, b"hello");
                    break;
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => continue,
                Err(e) => panic!("tcp write failed: {e:?}"),
            }
        }
    });

    let mut client = loop {
        let tcp = net::TcpStream::connect(addr);
        match tcp {
            Err(_) => continue,
            Ok(stream) => break stream,
        }
    };
    let mut buf = [0; 5];
    let ret = client.read(&mut buf).unwrap();
    assert_eq!(ret, 5);
    assert_eq!(&buf, b"hello");

    let ret = client.write(&buf).unwrap();
    assert_eq!(ret, 5);

    thread.join().unwrap();
}

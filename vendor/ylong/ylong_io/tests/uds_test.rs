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

#![cfg(target_os = "linux")]

use std::collections::HashMap;
use std::io;
use std::io::{IoSlice, IoSliceMut, Read, Write};
use std::net::Shutdown;
use std::os::fd::{FromRawFd, IntoRawFd};
use std::str::from_utf8;

use ylong_io::{EventTrait, Events, Interest, Poll, Token, UnixDatagram, UnixListener, UnixStream};

const PATH: &str = "/tmp/io_uds_path1";
const SERVER: Token = Token(0);

/// SDV test for UnixStream.
///
/// # Brief
/// 1. Create a pair of UnixStream.
/// 2. Server sends "Hello client".
/// 3. Client reads the message and sends "Hello server".
/// 4. Server receives the message
#[test]
fn sdv_uds_stream_test() {
    let _ = std::fs::remove_file(PATH);

    let handle = std::thread::spawn(server);

    let mut stream = loop {
        if let Ok(stream) = UnixStream::connect(PATH) {
            break stream;
        }
    };
    loop {
        let mut buffer = [0_u8; 1024];
        let slice = IoSliceMut::new(&mut buffer);
        std::thread::sleep(std::time::Duration::from_micros(300));
        match stream.read_vectored(&mut [slice]) {
            Ok(n) => {
                assert_eq!(from_utf8(&buffer[0..n]).unwrap(), "Hello client");
                break;
            }
            Err(_) => continue,
        }
    }

    let buf = b"Hello server";
    let slice = IoSlice::new(buf);
    let n = stream.write_vectored(&[slice]).unwrap();
    assert_eq!(n, 12);

    handle.join().unwrap().unwrap();
    stream.shutdown(Shutdown::Both).unwrap();
    std::fs::remove_file(PATH).unwrap();
}

fn server() -> io::Result<()> {
    let poll = Poll::new()?;
    let mut server = UnixListener::bind(PATH)?;

    poll.register(&mut server, SERVER, Interest::READABLE)?;
    let mut events = Events::with_capacity(128);
    // Map of `Token` -> `UnixListener`.
    let mut connections = HashMap::new();
    let mut unique_token = Token(SERVER.0 + 1);
    for _ in 0..3 {
        poll.poll(&mut events, None)?;

        for event in events.iter() {
            if SERVER == event.token() {
                let (mut stream, _) = server.accept()?;
                let token = Token(unique_token.0 + 1);
                unique_token = Token(unique_token.0 + 1);
                poll.register(&mut stream, token, Interest::READABLE | Interest::WRITABLE)?;
                connections.insert(token, stream);
            } else {
                match connections.get_mut(&event.token()) {
                    Some(connection) => {
                        if event.is_writable() {
                            match connection.write(b"Hello client") {
                                Err(_) => {
                                    poll.deregister(connection)?;
                                    poll.register(connection, event.token(), Interest::READABLE)?;
                                    break;
                                }
                                Ok(_) => {
                                    poll.deregister(connection)?;
                                    poll.register(connection, event.token(), Interest::READABLE)?;
                                    break;
                                }
                            }
                        } else if event.is_readable() {
                            let mut msg_buf = [0_u8; 100];
                            match connection.read(&mut msg_buf) {
                                Ok(0) => poll.deregister(connection)?,
                                Ok(n) => {
                                    if let Ok(str_buf) = from_utf8(&msg_buf[0..n]) {
                                        assert_eq!(str_buf, "Hello server");
                                    } else {
                                        println!("Received (none UTF-8) data: {:?}", &msg_buf);
                                    }
                                }
                                Err(_n) => {
                                    poll.deregister(connection)?;
                                    break;
                                }
                            }
                        }
                    }
                    None => break,
                }
            }
        }
    }
    Ok(())
}

/// SDV test for UnixDatagram.
///
/// # Brief
/// 1. Create a pair of UnixDatagram.
/// 2. Sender sends message first.
/// 3. Receiver receives message.
/// 4. Check if the test results are correct.
#[test]
fn sdv_uds_send_recv() {
    let (sender, _) = UnixDatagram::pair().unwrap();
    let addr = sender.local_addr().unwrap();
    let fmt = format!("{addr:?}");
    assert_eq!(&fmt, "(unnamed)");

    let addr = sender.peer_addr().unwrap();
    let fmt = format!("{addr:?}");
    assert_eq!(&fmt, "(unnamed)");

    let sender2 = sender.try_clone().unwrap();
    sender2.shutdown(Shutdown::Write).unwrap();
    let n = sender2.send(b"Hello");
    assert_eq!(n.unwrap_err().kind(), io::ErrorKind::BrokenPipe);

    let (sender, receiver) = UnixDatagram::pair().unwrap();
    let n = sender.send(b"Hello").expect("sender send failed");
    assert_eq!(n, "Hello".len());
    let mut buf = [0; 5];
    let ret = sender2.recv(&mut buf);
    assert!(ret.is_err());

    let mut recv_buf = [0_u8; 12];
    let fd = receiver.into_raw_fd();
    let receiver = unsafe { UnixDatagram::from_raw_fd(fd) };
    let len = loop {
        match receiver.recv_from(&mut recv_buf[..]) {
            Ok((n, addr)) => {
                let fmt = format!("{addr:?}");
                assert_eq!(&fmt, "(unnamed)");
                break n;
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(e) => panic!("{:?}", e),
        }
    };
    let fmt = format!("{receiver:?}");
    let expected = format!("fd: FileDesc(OwnedFd {{ fd: {fd} }})");
    assert!(fmt.contains(&expected));
    assert!(fmt.contains("local: (unnamed), peer: (unnamed)"));

    assert_eq!(&recv_buf[..len], b"Hello");
}

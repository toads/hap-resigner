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

use std::fmt::{Debug, Formatter};
use std::io;
use std::mem::MaybeUninit;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr};
use std::task::{Context, Poll};

use ylong_io::Interest;

use crate::io::ReadBuf;
use crate::net::sys::ToSocketAddrs;
use crate::net::AsyncSource;

/// Asynchronous UdpSocket.
///
/// # Examples
///
/// ```rust
/// use std::io;
///
/// use ylong_runtime::net::UdpSocket;
///
/// async fn io_func() -> io::Result<()> {
///     let sender_addr = "127.0.0.1:8081";
///     let receiver_addr = "127.0.0.1:8082";
///     let mut sender = UdpSocket::bind(sender_addr).await?;
///     let mut receiver = UdpSocket::bind(sender_addr).await?;
///
///     let len = sender.send_to(b"Hello", receiver_addr).await?;
///     println!("{:?} bytes sent", len);
///
///     let mut buf = [0; 1024];
///     let (len, addr) = receiver.recv_from(&mut buf).await?;
///     println!("{:?} bytes received from {:?}", len, addr);
///
///     let connected_sender = match sender.connect(receiver_addr).await {
///         Ok(socket) => socket,
///         Err(e) => {
///             return Err(e);
///         }
///     };
///     let connected_receiver = match receiver.connect(sender_addr).await {
///         Ok(socket) => socket,
///         Err(e) => {
///             return Err(e);
///         }
///     };
///     let len = connected_sender.send(b"Hello").await?;
///     println!("{:?} bytes sent", len);
///     let len = connected_receiver.recv(&mut buf).await?;
///     println!("{:?} bytes received from {:?}", len, sender_addr);
///     Ok(())
/// }
/// ```
pub struct UdpSocket {
    pub(crate) source: AsyncSource<ylong_io::UdpSocket>,
}

/// A connected asynchronous UdpSocket.
pub struct ConnectedUdpSocket {
    pub(crate) source: AsyncSource<ylong_io::ConnectedUdpSocket>,
}

impl Debug for UdpSocket {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.source.fmt(f)
    }
}

impl Debug for ConnectedUdpSocket {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        self.source.fmt(f)
    }
}

impl UdpSocket {
    /// Creates a new UDP socket and attempts to bind it to the address provided
    ///
    /// # Note
    ///
    /// If there are multiple addresses in SocketAddr, it will attempt to
    /// connect them in sequence until one of the addrs returns success. If
    /// all connections fail, it returns the error of the last connection.
    /// This behavior is consistent with std.
    ///
    /// # Panic
    /// Calling this method outside of a Ylong Runtime could cause panic.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let addr = "127.0.0.1:8080";
    ///     let mut sock = UdpSocket::bind(addr).await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<Self> {
        super::addr::each_addr(addr, ylong_io::UdpSocket::bind)
            .await
            .map(UdpSocket::new)
            .and_then(|op| op)
    }

    /// Internal interfaces.
    /// Creates new ylong_runtime::net::UdpSocket according to the incoming
    /// ylong_io::UdpSocket.
    pub(crate) fn new(socket: ylong_io::UdpSocket) -> io::Result<Self> {
        let source = AsyncSource::new(socket, None)?;
        Ok(UdpSocket { source })
    }

    /// Sets the default address for the UdpSocket and limits packets to
    /// those that are read via recv from the specific address.
    ///
    /// Returns the connected UdpSocket if succeeds.
    ///
    /// # Note
    ///
    /// If there are multiple addresses in SocketAddr, it will attempt to
    /// connect them in sequence until one of the addrs returns success. If
    /// all connections fail, it returns the error of the last connection.
    /// This behavior is consistent with std.
    ///
    /// # Panic
    /// Calling this method outside of a Ylong Runtime could cause panic.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let sock = UdpSocket::bind(local_addr).await?;
    ///     let remote_addr = "127.0.0.1:8081";
    ///     let connected_sock = match sock.connect(remote_addr).await {
    ///         Ok(socket) => socket,
    ///         Err(e) => {
    ///             return Err(e);
    ///         }
    ///     };
    ///     Ok(())
    /// }
    /// ```
    pub async fn connect<A: ToSocketAddrs>(self, addr: A) -> io::Result<ConnectedUdpSocket> {
        let local_addr = self.local_addr()?;
        drop(self);

        let addrs = addr.to_socket_addrs().await?;

        let mut last_e = None;

        for addr in addrs {
            let socket = ylong_io::UdpSocket::bind(local_addr)?;
            match socket.connect(addr) {
                Ok(socket) => return ConnectedUdpSocket::new(socket),
                Err(e) => last_e = Some(e),
            }
        }

        Err(last_e.unwrap_or(io::Error::new(
            io::ErrorKind::InvalidInput,
            "addr could not resolve to any address",
        )))
    }

    /// Returns the local address that this socket is bound to.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let addr = "127.0.0.1:8080";
    ///     let mut sock = UdpSocket::bind(addr).await?;
    ///     let local_addr = sock.local_addr()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.source.local_addr()
    }

    /// Sends data on the socket to the given address. On success, returns the
    /// number of bytes written. This will return an error when the IP
    /// version of the local socket does not match that returned from
    /// SocketAddr.
    ///
    /// # Return value
    /// The function returns:
    /// * `Ok(n)` n is the number of bytes sent.
    /// * `Err(e)` if an error is encountered.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let sock = UdpSocket::bind(local_addr).await?;
    ///     let remote_addr = "127.0.0.1:8081";
    ///     let len = sock.send_to(b"hello world", remote_addr).await?;
    ///     println!("Sent {} bytes", len);
    ///     Ok(())
    /// }
    /// ```
    pub async fn send_to<A: ToSocketAddrs>(&self, buf: &[u8], target: A) -> io::Result<usize> {
        match target.to_socket_addrs().await?.next() {
            Some(addr) => {
                self.source
                    .async_process(Interest::WRITABLE, || self.source.send_to(buf, addr))
                    .await
            }
            None => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "addr could not resolve to address",
            )),
        }
    }

    /// Attempts to send data on the socket to the given address.
    ///
    /// The function is usually paired with `writable`.
    ///
    /// # Return value
    /// The function returns:
    /// * `Ok(n)` n is the number of bytes sent.
    /// * `Err(e)` if an error is encountered.
    /// When the remote cannot receive the message, an
    /// [`io::ErrorKind::WouldBlock`] will be returned. This will return an
    /// error If the IP version of the local socket does not match that
    /// returned from SocketAddr.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let sock = UdpSocket::bind(local_addr).await?;
    ///     let remote_addr = "127.0.0.1:8081".parse().unwrap();
    ///     let len = sock.try_send_to(b"hello world", remote_addr)?;
    ///     Ok(())
    /// }
    /// ```
    pub fn try_send_to(&self, buf: &[u8], target: SocketAddr) -> io::Result<usize> {
        self.source
            .try_io(Interest::WRITABLE, || self.source.send_to(buf, target))
    }

    /// Attempts to send data on the socket to a given address.
    /// Note that on multiple calls to a poll_* method in the send direction,
    /// only the Waker from the Context passed to the most recent call will be
    /// scheduled to receive a wakeup
    ///
    /// # Return value
    /// The function returns:
    /// * `Poll::Pending` if the socket is not ready to write
    /// * `Poll::Ready(Ok(n))` n is the number of bytes sent.
    /// * `Poll::Ready(Err(e))` if an error is encountered.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::futures::poll_fn;
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let sock = UdpSocket::bind(local_addr).await?;
    ///     let remote_addr = "127.0.0.1:8081".parse().unwrap();
    ///     let len = poll_fn(|cx| sock.poll_send_to(cx, b"Hello", remote_addr)).await?;
    ///     println!("Sent {} bytes", len);
    ///     Ok(())
    /// }
    /// ```
    pub fn poll_send_to(
        &self,
        cx: &mut Context<'_>,
        buf: &[u8],
        target: SocketAddr,
    ) -> Poll<io::Result<usize>> {
        self.source
            .poll_write_io(cx, || self.source.send_to(buf, target))
    }

    /// Receives a single datagram message on the socket. On success, returns
    /// the number of bytes read and the origin. The function must be called
    /// with valid byte array buf of sufficient size to hold the message
    /// bytes. If a message is too long to fit in the supplied buffer,
    /// excess bytes may be discarded.
    ///
    /// # Return value
    /// The function returns:
    /// * `Ok((n, addr))` n is the number of bytes received, addr is the address
    ///   of sender.
    /// * `Err(e)` if an error is encountered.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let sock = UdpSocket::bind(local_addr).await?;
    ///     let mut recv_buf = [0_u8; 12];
    ///     let (len, addr) = sock.recv_from(&mut recv_buf).await?;
    ///     println!("received {:?} bytes from {:?}", len, addr);
    ///     Ok(())
    /// }
    /// ```
    pub async fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.source
            .async_process(Interest::READABLE, || self.source.recv_from(buf))
            .await
    }

    /// Attempts to receive a single datagram message on the socket.
    ///
    /// The function is usually paired with `readable` and must be called with
    /// valid byte array buf of sufficient size to hold the message bytes.
    /// If a message is too long to fit in the supplied buffer, excess bytes
    /// may be discarded.
    ///
    /// # Return value
    /// The function returns:
    /// * `Ok(n, addr)` n is the number of bytes received, addr is the address
    ///   of the remote.
    /// * `Err(e)` if an error is encountered.
    /// If there is no pending data, an [`io::ErrorKind::WouldBlock`] will be
    /// returned.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let sock = UdpSocket::bind(local_addr).await?;
    ///     let mut recv_buf = [0_u8; 12];
    ///     let (len, addr) = sock.try_recv_from(&mut recv_buf)?;
    ///     Ok(())
    /// }
    /// ```
    pub fn try_recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.source
            .try_io(Interest::READABLE, || self.source.recv_from(buf))
    }

    /// Attempts to receives single datagram on the socket from the remote
    /// address to which it is connected, without removing the message from
    /// input queue. On success, returns the number of bytes peeked.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let sock = UdpSocket::bind(local_addr).await?;
    ///     let mut buf = [0; 10];
    ///     let (number_of_bytes, src_addr) =
    ///         sock.peek_from(&mut buf).await.expect("Didn't receive data");
    ///     let filled_buf = &mut buf[..number_of_bytes];
    ///     Ok(())
    /// }
    /// ```
    pub async fn peek_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.source
            .async_process(Interest::READABLE, || self.source.peek_from(buf))
            .await
    }

    /// Attempts to Receives single datagram on the socket from the remote
    /// address to which it is connected, without removing the message from
    /// input queue. On success, returns the number of bytes peeked.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let sock = UdpSocket::bind(local_addr).await?;
    ///     let mut buf = [0; 10];
    ///     let (number_of_bytes, src_addr) =
    ///         sock.try_peek_from(&mut buf).expect("Didn't receive data");
    ///     Ok(())
    /// }
    /// ```
    pub fn try_peek_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        self.source
            .try_io(Interest::READABLE, || self.source.peek_from(buf))
    }

    /// Attempts to receives single datagram on the socket from the remote
    /// address to which it is connected, without removing the message from
    /// input queue. On success, returns the number of bytes peeked.
    ///
    /// # Return value
    /// The function returns:
    /// * `Poll::Pending` if the socket is not ready to read
    /// * `Poll::Ready(Ok(addr))` reads data from addr into ReadBuf if the
    ///   socket is ready
    /// * `Poll::Ready(Err(e))` if an error is encountered.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::futures::poll_fn;
    /// use ylong_runtime::io::ReadBuf;
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let sock = UdpSocket::bind(local_addr).await?;
    ///     let mut recv_buf = [0_u8; 12];
    ///     let mut read = ReadBuf::new(&mut recv_buf);
    ///     let addr = poll_fn(|cx| sock.poll_peek_from(cx, &mut read)).await?;
    ///     println!("received {:?} from {:?}", read.filled(), addr);
    ///     Ok(())
    /// }
    /// ```
    pub fn poll_peek_from(
        &self,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<SocketAddr>> {
        let ret = self.source.poll_read_io(cx, || unsafe {
            let slice = &mut *(buf.unfilled_mut() as *mut [MaybeUninit<u8>] as *mut [u8]);
            self.source.peek_from(slice)
        });
        let (r_len, r_addr) = match ret {
            Poll::Ready(Ok(x)) => x,
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Pending => return Poll::Pending,
        };
        buf.assume_init(r_len);
        buf.advance(r_len);

        Poll::Ready(Ok(r_addr))
    }

    /// Waits for the socket to become readable.
    ///
    /// This function is usually paired up with [`UdpSocket::try_recv_from`]
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let sock = UdpSocket::bind(local_addr).await?;
    ///     sock.readable().await?;
    ///     let mut buf = [0; 12];
    ///     let (len, addr) = sock.try_recv_from(&mut buf)?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn readable(&self) -> io::Result<()> {
        self.source.entry.readiness(Interest::READABLE).await?;
        Ok(())
    }

    /// Waits for the socket to become writable.
    ///
    /// This function is usually paired up with [`UdpSocket::try_send_to`]
    ///
    /// # Examples
    /// ```
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let remote_addr = "127.0.0.1:8080".parse().unwrap();
    ///     let sock = UdpSocket::bind(local_addr).await?;
    ///     sock.writable().await?;
    ///     let len = sock.try_send_to(b"hello", remote_addr)?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn writable(&self) -> io::Result<()> {
        self.source.entry.readiness(Interest::WRITABLE).await?;
        Ok(())
    }

    /// Attempts to receive a single datagram on the socket.
    /// Note that on multiple calls to a poll_* method in the recv direction,
    /// only the Waker from the Context passed to the most recent call will be
    /// scheduled to receive a wakeup.
    ///
    /// # Return value
    /// The function returns:
    /// * `Poll::Pending` if the socket is not ready to read
    /// * `Poll::Ready(Ok(addr))` reads data from addr into ReadBuf if the
    ///   socket is ready
    /// * `Poll::Ready(Err(e))` if an error is encountered.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::futures::poll_fn;
    /// use ylong_runtime::io::ReadBuf;
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let sock = UdpSocket::bind(local_addr).await?;
    ///     let mut recv_buf = [0_u8; 12];
    ///     let mut read = ReadBuf::new(&mut recv_buf);
    ///     let addr = poll_fn(|cx| sock.poll_recv_from(cx, &mut read)).await?;
    ///     println!("received {:?} from {:?}", read.filled(), addr);
    ///     Ok(())
    /// }
    /// ```
    pub fn poll_recv_from(
        &self,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<SocketAddr>> {
        let ret = self.source.poll_read_io(cx, || unsafe {
            let slice = &mut *(buf.unfilled_mut() as *mut [MaybeUninit<u8>] as *mut [u8]);
            self.source.recv_from(slice)
        });
        let (r_len, r_addr) = match ret {
            Poll::Ready(Ok(x)) => x,
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Pending => return Poll::Pending,
        };
        buf.assume_init(r_len);
        buf.advance(r_len);

        Poll::Ready(Ok(r_addr))
    }

    /// Sets the value of the `SO_BROADCAST` option for this socket.
    /// When enabled, this socket is allowed to send packets to a broadcast
    /// address.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let broadcast_socket = UdpSocket::bind(local_addr).await?;
    ///     if broadcast_socket.broadcast()? == false {
    ///         broadcast_socket.set_broadcast(true)?;
    ///     }
    ///     assert_eq!(broadcast_socket.broadcast()?, true);
    ///     Ok(())
    /// }
    /// ```
    pub fn set_broadcast(&self, on: bool) -> io::Result<()> {
        self.source.set_broadcast(on)
    }

    /// Gets the value of the `SO_BROADCAST` option for this socket.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let broadcast_socket = UdpSocket::bind(local_addr).await?;
    ///     assert_eq!(broadcast_socket.broadcast()?, false);
    ///     Ok(())
    /// }
    /// ```
    pub fn broadcast(&self) -> io::Result<bool> {
        self.source.broadcast()
    }

    /// Sets the value for the IP_TTL option on this socket.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let socket = UdpSocket::bind(local_addr).await?;
    ///     socket.set_ttl(100).expect("set_ttl call failed");
    ///     Ok(())
    /// }
    /// ```
    pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        self.source.set_ttl(ttl)
    }

    /// Sets the value for the IP_TTL option on this socket.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let socket = UdpSocket::bind(local_addr).await?;
    ///     socket.set_ttl(100).expect("set_ttl call failed");
    ///     assert_eq!(socket.ttl().unwrap(), 100);
    ///     Ok(())
    /// }
    /// ```
    pub fn ttl(&self) -> io::Result<u32> {
        self.source.ttl()
    }

    /// Gets the value of the SO_ERROR option on this socket.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let addr = "127.0.0.1:34254";
    ///     let socket = UdpSocket::bind(addr)
    ///         .await
    ///         .expect("couldn't bind to address");
    ///     match socket.take_error() {
    ///         Ok(Some(error)) => println!("UdpSocket error: {error:?}"),
    ///         Ok(None) => println!("No error"),
    ///         Err(error) => println!("UdpSocket.take_error failed: {error:?}"),
    ///     }
    ///     Ok(())
    /// }
    /// ```
    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        self.source.take_error()
    }

    /// Gets the value of the IP_MULTICAST_LOOP option for this socket.
    pub fn multicast_loop_v4(&self) -> io::Result<bool> {
        self.source.multicast_loop_v4()
    }

    /// Sets the value of the IP_MULTICAST_LOOP option for this socket.
    /// If enabled, multicast packets will be looped back to the local socket.
    /// Note that this might not have any effect on IPv6 sockets.
    pub fn set_multicast_loop_v4(&self, multicast_loop_v4: bool) -> io::Result<()> {
        self.source.set_multicast_loop_v4(multicast_loop_v4)
    }

    /// Gets the value of the IP_MULTICAST_TTL option for this socket.
    pub fn multicast_ttl_v4(&self) -> io::Result<u32> {
        self.source.multicast_ttl_v4()
    }

    /// Sets the value of the IP_MULTICAST_TTL option for this socket.
    /// Indicates the time-to-live value of outgoing multicast packets for this
    /// socket. The default value is 1 which means that multicast packets don't
    /// leave the local network unless explicitly requested. Note that this
    /// might not have any effect on IPv6 sockets.
    pub fn set_multicast_ttl_v4(&self, multicast_ttl_v4: u32) -> io::Result<()> {
        self.source.set_multicast_ttl_v4(multicast_ttl_v4)
    }

    /// Gets the value of the IPV6_MULTICAST_LOOP option for this socket.
    pub fn multicast_loop_v6(&self) -> io::Result<bool> {
        self.source.multicast_loop_v6()
    }

    /// Sets the value of the IPV6_MULTICAST_LOOP option for this socket.
    /// Controls whether this socket sees the multicast packets it sends itself.
    /// Note that this might not have any affect on IPv4 sockets.
    pub fn set_multicast_loop_v6(&self, multicast_loop_v6: bool) -> io::Result<()> {
        self.source.set_multicast_loop_v6(multicast_loop_v6)
    }

    /// Executes an operation of the IP_ADD_MEMBERSHIP type.
    pub fn join_multicast_v4(&self, multiaddr: &Ipv4Addr, interface: &Ipv4Addr) -> io::Result<()> {
        self.source.join_multicast_v4(multiaddr, interface)
    }

    /// Executes an operation of the IPV6_ADD_MEMBERSHIP type.
    pub fn join_multicast_v6(&self, multiaddr: &Ipv6Addr, interface: u32) -> io::Result<()> {
        self.source.join_multicast_v6(multiaddr, interface)
    }

    /// Executes an operation of the IP_DROP_MEMBERSHIP type.
    pub fn leave_multicast_v4(&self, multiaddr: &Ipv4Addr, interface: &Ipv4Addr) -> io::Result<()> {
        self.source.leave_multicast_v4(multiaddr, interface)
    }

    /// Executes an operation of the IPV6_DROP_MEMBERSHIP type.
    pub fn leave_multicast_v6(&self, multiaddr: &Ipv6Addr, interface: u32) -> io::Result<()> {
        self.source.leave_multicast_v6(multiaddr, interface)
    }
}

impl ConnectedUdpSocket {
    /// Internal interfaces.
    /// Creates new ylong_runtime::net::ConnectedUdpSocket according to the
    /// incoming ylong_io::UdpSocket.
    pub(crate) fn new(socket: ylong_io::ConnectedUdpSocket) -> io::Result<Self> {
        let source = AsyncSource::new(socket, None)?;
        Ok(ConnectedUdpSocket { source })
    }

    /// Returns the local address that this socket is bound to.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let addr = "127.0.0.1:8080";
    ///     let mut sock = UdpSocket::bind(addr).await?;
    ///     let remote_addr = "127.0.0.1:8081";
    ///     let connected_sock = match sock.connect(remote_addr).await {
    ///         Ok(socket) => socket,
    ///         Err(e) => {
    ///             return Err(e);
    ///         }
    ///     };
    ///     let local_addr = connected_sock.local_addr()?;
    ///     Ok(())
    /// }
    /// ```
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.source.local_addr()
    }

    /// Returns the socket address of the remote peer this socket was connected
    /// to.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let addr = "127.0.0.1:8080";
    ///     let peer_addr = "127.0.0.1:8081";
    ///     let mut sock = UdpSocket::bind(addr).await?;
    ///     let connected_sock = match sock.connect(peer_addr).await {
    ///         Ok(socket) => socket,
    ///         Err(e) => {
    ///             return Err(e);
    ///         }
    ///     };
    ///     assert_eq!(connected_sock.peer_addr()?, peer_addr.parse().unwrap());
    ///     Ok(())
    /// }
    /// ```
    pub fn peer_addr(&self) -> io::Result<SocketAddr> {
        self.source.peer_addr()
    }

    /// Sets the value of the `SO_BROADCAST` option for this socket.
    /// When enabled, this socket is allowed to send packets to a broadcast
    /// address.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let peer_addr = "127.0.0.1:8081";
    ///     let socket = UdpSocket::bind(local_addr).await?;
    ///     let connected_sock = match socket.connect(peer_addr).await {
    ///         Ok(socket) => socket,
    ///         Err(e) => {
    ///             return Err(e);
    ///         }
    ///     };
    ///     if connected_sock.broadcast()? == false {
    ///         connected_sock.set_broadcast(true)?;
    ///     }
    ///     assert_eq!(connected_sock.broadcast()?, true);
    ///     Ok(())
    /// }
    /// ```
    pub fn set_broadcast(&self, on: bool) -> io::Result<()> {
        self.source.set_broadcast(on)
    }

    /// Gets the value of the `SO_BROADCAST` option for this socket.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let peer_addr = "127.0.0.1:8081";
    ///     let socket = UdpSocket::bind(local_addr).await?;
    ///     let connected_sock = match socket.connect(peer_addr).await {
    ///         Ok(socket) => socket,
    ///         Err(e) => {
    ///             return Err(e);
    ///         }
    ///     };
    ///     assert_eq!(connected_sock.broadcast()?, false);
    ///     Ok(())
    /// }
    /// ```
    pub fn broadcast(&self) -> io::Result<bool> {
        self.source.broadcast()
    }

    /// Sets the value for the IP_TTL option on this socket.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let peer_addr = "127.0.0.1:8081";
    ///     let socket = UdpSocket::bind(local_addr).await?;
    ///     let connect_socket = socket.connect(peer_addr).await?;
    ///     connect_socket.set_ttl(100).expect("set_ttl call failed");
    ///     Ok(())
    /// }
    /// ```
    pub fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        self.source.set_ttl(ttl)
    }

    /// Sets the value for the IP_TTL option on this socket.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let peer_addr = "127.0.0.1:8081";
    ///     let socket = UdpSocket::bind(local_addr).await?;
    ///     let connect_socket = socket.connect(peer_addr).await?;
    ///     connect_socket.set_ttl(100).expect("set_ttl call failed");
    ///     assert_eq!(connect_socket.ttl().unwrap(), 100);
    ///     Ok(())
    /// }
    /// ```
    pub fn ttl(&self) -> io::Result<u32> {
        self.source.ttl()
    }

    /// Sends data on the socket to the remote address that the socket is
    /// connected to. The connect method will connect this socket to a
    /// remote address. This method will fail if the socket is not
    /// connected.
    ///
    /// # Return value
    /// On success, the number of bytes sent is returned, otherwise, the
    /// encountered error is returned.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let sock = UdpSocket::bind(local_addr).await?;
    ///     let remote_addr = "127.0.0.1:8081";
    ///     let connected_sock = match sock.connect(remote_addr).await {
    ///         Ok(socket) => socket,
    ///         Err(e) => {
    ///             return Err(e);
    ///         }
    ///     };
    ///     connected_sock.send(b"Hello").await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn send(&self, buf: &[u8]) -> io::Result<usize> {
        self.source
            .async_process(Interest::WRITABLE, || self.source.send(buf))
            .await
    }

    /// Attempts to send data on the socket to the remote address that the
    /// socket is connected to. This method will fail if the socket is not
    /// connected.
    ///
    /// The function is usually paired with `writable`.
    ///
    /// # Return value
    /// The function returns:
    /// * `Ok(n)` n is the number of bytes sent.
    /// * `Err(e)` if an error is encountered.
    /// When the remote cannot receive the message, an
    /// [`io::ErrorKind::WouldBlock`] will be returned.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let sock = UdpSocket::bind(local_addr).await?;
    ///     let remote_addr = "127.0.0.1:8081";
    ///     let connected_sock = match sock.connect(remote_addr).await {
    ///         Ok(socket) => socket,
    ///         Err(e) => {
    ///             return Err(e);
    ///         }
    ///     };
    ///     connected_sock.try_send(b"Hello")?;
    ///     Ok(())
    /// }
    /// ```
    pub fn try_send(&self, buf: &[u8]) -> io::Result<usize> {
        self.source
            .try_io(Interest::WRITABLE, || self.source.send(buf))
    }

    /// Attempts to send data on the socket to the remote address to which it
    /// was previously connected. The connect method will connect this
    /// socket to a remote address. This method will fail if the socket is
    /// not connected. Note that on multiple calls to a poll_* method in the
    /// send direction, only the Waker from the Context passed to the most
    /// recent call will be scheduled to receive a wakeup.
    ///
    /// # Return value
    /// The function returns:
    /// * `Poll::Pending` if the socket is not available to write
    /// * `Poll::Ready(Ok(n))` `n` is the number of bytes sent
    /// * `Poll::Ready(Err(e))` if an error is encountered.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::futures::poll_fn;
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let sock = UdpSocket::bind(local_addr).await?;
    ///     let remote_addr = "127.0.0.1:8081";
    ///     let connected_sock = match sock.connect(remote_addr).await {
    ///         Ok(socket) => socket,
    ///         Err(e) => {
    ///             return Err(e);
    ///         }
    ///     };
    ///     poll_fn(|cx| connected_sock.poll_send(cx, b"Hello")).await?;
    ///     Ok(())
    /// }
    /// ```
    pub fn poll_send(&self, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        self.source.poll_write_io(cx, || self.source.send(buf))
    }

    /// Receives a single datagram message on the socket from the remote address
    /// to which it is connected. On success, returns the number of bytes
    /// read. The function must be called with valid byte array buf of
    /// sufficient size to hold the message bytes. If a message is too long
    /// to fit in the supplied buffer, excess bytes may be discarded.
    /// The connect method will connect this socket to a remote address.
    /// This method will fail if the socket is not connected.
    ///
    /// # Return value
    /// The function returns:
    /// * `Ok(n)` n is is the number of bytes received
    /// * `Err(e)` if an error is encountered.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let sock = UdpSocket::bind(local_addr).await?;
    ///     let remote_addr = "127.0.0.1:8081";
    ///     let connected_sock = match sock.connect(remote_addr).await {
    ///         Ok(socket) => socket,
    ///         Err(e) => {
    ///             return Err(e);
    ///         }
    ///     };
    ///     let mut recv_buf = [0_u8; 12];
    ///     let n = connected_sock.recv(&mut recv_buf[..]).await?;
    ///     println!("received {} bytes {:?}", n, &recv_buf[..n]);
    ///     Ok(())
    /// }
    /// ```
    pub async fn recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.source
            .async_process(Interest::READABLE, || self.source.recv(buf))
            .await
    }

    /// Attempts to receive a single datagram message on the socket from the
    /// remote address to which it is connected.
    /// On success, returns the number of bytes read. The function must be
    /// called with valid byte array buf of sufficient size to hold the
    /// message bytes. If a message is too long to fit in the supplied
    /// buffer, excess bytes may be discarded. This method will fail if the
    /// socket is not connected.
    ///
    /// # Return value
    /// The function returns:
    /// * `Ok(n, addr)` n is the number of bytes received, addr is the address
    ///   of the remote.
    /// * `Err(e)` if an error is encountered.
    /// If there is no pending data, an [`io::ErrorKind::WouldBlock`] will be
    /// returned.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let sock = UdpSocket::bind(local_addr).await?;
    ///     let remote_addr = "127.0.0.1:8081";
    ///     let connected_sock = match sock.connect(remote_addr).await {
    ///         Ok(socket) => socket,
    ///         Err(e) => {
    ///             return Err(e);
    ///         }
    ///     };
    ///     let mut recv_buf = [0_u8; 12];
    ///     let n = connected_sock.try_recv(&mut recv_buf[..])?;
    ///     println!("received {} bytes {:?}", n, &recv_buf[..n]);
    ///     Ok(())
    /// }
    /// ```
    pub fn try_recv(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.source
            .try_io(Interest::READABLE, || self.source.recv(buf))
    }

    /// Attempts to receive a single datagram message on the socket from the
    /// remote address to which it is connected. The connect method will
    /// connect this socket to a remote address. This method resolves to an
    /// error if the socket is not connected. Note that on multiple calls to
    /// a poll_* method in the recv direction, only the Waker from the
    /// Context passed to the most recent call will be scheduled to receive a
    /// wakeup.
    ///
    /// # Return value
    /// The function returns:
    ///
    /// * `Poll::Pending` if the socket is not ready to read
    /// * `Poll::Ready(Ok(()))` reads data ReadBuf if the socket is ready
    /// * `Poll::Ready(Err(e))` if an error is encountered.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::futures::poll_fn;
    /// use ylong_runtime::io::ReadBuf;
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let sock = UdpSocket::bind(local_addr).await?;
    ///     let remote_addr = "127.0.0.1:8081";
    ///     let connected_sock = match sock.connect(remote_addr).await {
    ///         Ok(socket) => socket,
    ///         Err(e) => {
    ///             return Err(e);
    ///         }
    ///     };
    ///     let mut recv_buf = [0_u8; 12];
    ///     let mut read = ReadBuf::new(&mut recv_buf);
    ///     poll_fn(|cx| connected_sock.poll_recv(cx, &mut read)).await?;
    ///     println!("received : {:?}", read.filled());
    ///     Ok(())
    /// }
    /// ```
    pub fn poll_recv(&self, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let ret = self.source.poll_read_io(cx, || unsafe {
            let slice = &mut *(buf.unfilled_mut() as *mut [MaybeUninit<u8>] as *mut [u8]);
            self.source.recv(slice)
        });
        let r_len = match ret {
            Poll::Ready(Ok(x)) => x,
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
            Poll::Pending => return Poll::Pending,
        };
        buf.assume_init(r_len);
        buf.advance(r_len);

        Poll::Ready(Ok(()))
    }

    /// Receives single datagram on the socket from the remote address to which
    /// it is connected, without removing the message from input queue. On
    /// success, returns the number of bytes peeked.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let receiver_addr = "127.0.0.1:8081";
    ///     let socket = UdpSocket::bind(local_addr).await?;
    ///     let connect_socket = socket
    ///         .connect(receiver_addr)
    ///         .await
    ///         .expect("connect function failed");
    ///     let mut buf = [0; 10];
    ///     match connect_socket.peek(&mut buf).await {
    ///         Ok(received) => println!("received {received} bytes"),
    ///         Err(e) => println!("peek function failed: {e:?}"),
    ///     }
    ///     Ok(())
    /// }
    /// ```
    pub async fn peek(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.source
            .async_process(Interest::READABLE, || self.source.peek(buf))
            .await
    }

    /// Attempts to Receives single datagram on the socket from the remote
    /// address to which it is connected, without removing the message from
    /// input queue. On success, returns the number of bytes peeked.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let receiver_addr = "127.0.0.1:8081";
    ///     let socket = UdpSocket::bind(local_addr).await?;
    ///     let connect_socket = socket
    ///         .connect(receiver_addr)
    ///         .await
    ///         .expect("connect function failed");
    ///     let mut buf = [0; 10];
    ///     match connect_socket.try_peek(&mut buf) {
    ///         Ok(received) => println!("received {received} bytes"),
    ///         Err(e) => println!("try_peek function failed: {e:?}"),
    ///     }
    ///     Ok(())
    /// }
    /// ```
    pub fn try_peek(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.source
            .try_io(Interest::READABLE, || self.source.peek(buf))
    }

    /// Waits for the socket to become readable.
    ///
    /// This function is usually paired up with [`UdpSocket::try_recv_from`]
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io;
    ///
    /// use ylong_runtime::net::{ConnectedUdpSocket, UdpSocket};
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let sock = UdpSocket::bind(local_addr).await?;
    ///     let remote_addr = "127.0.0.1:8081";
    ///     let connected_sock = match sock.connect(remote_addr).await {
    ///         Ok(socket) => socket,
    ///         Err(e) => {
    ///             return Err(e);
    ///         }
    ///     };
    ///     connected_sock.readable().await?;
    ///     let mut buf = [0; 12];
    ///     let len = connected_sock.try_recv(&mut buf)?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn readable(&self) -> io::Result<()> {
        self.source.entry.readiness(Interest::READABLE).await?;
        Ok(())
    }

    /// Waits for the socket to become writable.
    ///
    /// This function is usually paired up with [`UdpSocket::try_send_to`]
    ///
    /// # Examples
    ///
    /// ```
    /// use std::io;
    ///
    /// use ylong_runtime::net::{ConnectedUdpSocket, UdpSocket};
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let sock = UdpSocket::bind(local_addr).await?;
    ///     let remote_addr = "127.0.0.1:8081";
    ///     let connected_sock = match sock.connect(remote_addr).await {
    ///         Ok(socket) => socket,
    ///         Err(e) => {
    ///             return Err(e);
    ///         }
    ///     };
    ///     connected_sock.writable().await?;
    ///     let mut buf = [0; 12];
    ///     let len = connected_sock.try_send(&mut buf)?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn writable(&self) -> io::Result<()> {
        self.source.entry.readiness(Interest::WRITABLE).await?;
        Ok(())
    }

    /// Gets the value of the SO_ERROR option on this socket.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::io;
    ///
    /// use ylong_runtime::net::UdpSocket;
    ///
    /// async fn io_func() -> io::Result<()> {
    ///     let local_addr = "127.0.0.1:8080";
    ///     let socket = UdpSocket::bind(local_addr)
    ///         .await
    ///         .expect("couldn't bind to address");
    ///     let remote_addr = "127.0.0.1:8081";
    ///     let connected_sock = match socket.connect(remote_addr).await {
    ///         Ok(socket) => socket,
    ///         Err(e) => {
    ///             return Err(e);
    ///         }
    ///     };
    ///     match connected_sock.take_error() {
    ///         Ok(Some(error)) => println!("UdpSocket error: {error:?}"),
    ///         Ok(None) => println!("No error"),
    ///         Err(error) => println!("UdpSocket.take_error failed: {error:?}"),
    ///     }
    ///     Ok(())
    /// }
    /// ```
    pub fn take_error(&self) -> io::Result<Option<io::Error>> {
        self.source.take_error()
    }

    /// Gets the value of the IP_MULTICAST_LOOP option for this socket.
    pub fn multicast_loop_v4(&self) -> io::Result<bool> {
        self.source.multicast_loop_v4()
    }

    /// Sets the value of the IP_MULTICAST_LOOP option for this socket.
    /// If enabled, multicast packets will be looped back to the local socket.
    /// Note that this might not have any effect on IPv6 sockets.
    pub fn set_multicast_loop_v4(&self, multicast_loop_v4: bool) -> io::Result<()> {
        self.source.set_multicast_loop_v4(multicast_loop_v4)
    }

    /// Gets the value of the IP_MULTICAST_TTL option for this socket.
    pub fn multicast_ttl_v4(&self) -> io::Result<u32> {
        self.source.multicast_ttl_v4()
    }

    /// Sets the value of the IP_MULTICAST_TTL option for this socket.
    /// Indicates the time-to-live value of outgoing multicast packets for this
    /// socket. The default value is 1 which means that multicast packets don't
    /// leave the local network unless explicitly requested. Note that this
    /// might not have any effect on IPv6 sockets.
    pub fn set_multicast_ttl_v4(&self, multicast_ttl_v4: u32) -> io::Result<()> {
        self.source.set_multicast_ttl_v4(multicast_ttl_v4)
    }

    /// Gets the value of the IPV6_MULTICAST_LOOP option for this socket.
    pub fn multicast_loop_v6(&self) -> io::Result<bool> {
        self.source.multicast_loop_v6()
    }

    /// Sets the value of the IPV6_MULTICAST_LOOP option for this socket.
    /// Controls whether this socket sees the multicast packets it sends itself.
    /// Note that this might not have any affect on IPv4 sockets.
    pub fn set_multicast_loop_v6(&self, multicast_loop_v6: bool) -> io::Result<()> {
        self.source.set_multicast_loop_v6(multicast_loop_v6)
    }

    /// Executes an operation of the IP_ADD_MEMBERSHIP type.
    pub fn join_multicast_v4(&self, multiaddr: &Ipv4Addr, interface: &Ipv4Addr) -> io::Result<()> {
        self.source.join_multicast_v4(multiaddr, interface)
    }

    /// Executes an operation of the IPV6_ADD_MEMBERSHIP type.
    pub fn join_multicast_v6(&self, multiaddr: &Ipv6Addr, interface: u32) -> io::Result<()> {
        self.source.join_multicast_v6(multiaddr, interface)
    }

    /// Executes an operation of the IP_DROP_MEMBERSHIP type.
    pub fn leave_multicast_v4(&self, multiaddr: &Ipv4Addr, interface: &Ipv4Addr) -> io::Result<()> {
        self.source.leave_multicast_v4(multiaddr, interface)
    }

    /// Executes an operation of the IPV6_DROP_MEMBERSHIP type.
    pub fn leave_multicast_v6(&self, multiaddr: &Ipv6Addr, interface: u32) -> io::Result<()> {
        self.source.leave_multicast_v6(multiaddr, interface)
    }
}

#[cfg(windows)]
use std::os::windows::io::{AsRawSocket, RawSocket};

#[cfg(windows)]
impl AsRawSocket for UdpSocket {
    fn as_raw_socket(&self) -> RawSocket {
        self.source.as_raw_socket()
    }
}

#[cfg(windows)]
impl AsRawSocket for ConnectedUdpSocket {
    fn as_raw_socket(&self) -> RawSocket {
        self.source.as_raw_socket()
    }
}

#[cfg(unix)]
use std::os::fd::{AsRawFd, RawFd};

#[cfg(unix)]
use ylong_io::Source;

#[cfg(unix)]
impl AsRawFd for UdpSocket {
    fn as_raw_fd(&self) -> RawFd {
        self.source.get_fd()
    }
}

#[cfg(unix)]
impl AsRawFd for ConnectedUdpSocket {
    fn as_raw_fd(&self) -> RawFd {
        self.source.get_fd()
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::net::{Ipv4Addr, Ipv6Addr};

    use crate::futures::poll_fn;
    use crate::io::ReadBuf;
    use crate::net::{ConnectedUdpSocket, UdpSocket};
    use crate::{block_on, spawn};

    const ADDR: &str = "127.0.0.1:0";

    // Try to bind a udp in local_addr and connect to peer_addr, Either of the two
    // has an AddrInUse error will retry until success.
    async fn udp_try_bind_connect<F>(
        addr: &str,
        mut f: F,
    ) -> io::Result<(ConnectedUdpSocket, ConnectedUdpSocket)>
    where
        F: FnMut(&UdpSocket),
    {
        loop {
            let local_udp_socket = UdpSocket::bind(addr).await?;
            let peer_udp_socket = UdpSocket::bind(addr).await?;

            f(&local_udp_socket);

            let local_addr = local_udp_socket.local_addr().unwrap();
            let peer_addr = peer_udp_socket.local_addr().unwrap();
            let local_connect = match local_udp_socket.connect(peer_addr).await {
                Ok(socket) => socket,
                Err(e) if e.kind() == io::ErrorKind::AddrInUse => continue,
                Err(e) => return Err(e),
            };
            let peer_connect = match peer_udp_socket.connect(local_addr).await {
                Ok(socket) => socket,
                Err(e) if e.kind() == io::ErrorKind::AddrInUse => continue,
                Err(e) => return Err(e),
            };
            return Ok((local_connect, peer_connect));
        }
    }

    /// Basic UT test cases for `UdpSocket` with `SocketAddrV4`.
    ///
    /// # Brief
    /// 1. Bind and connect `UdpSocket`.
    /// 2. Call set_ttl(), ttl(), take_error(), set_multicast_loop_v4(),
    ///    multicast_loop_v4(), set_multicast_ttl_v4(), multicast_ttl_v4() for
    ///    `UdpSocket` and `ConnectedUdpSocket`.
    /// 3. Check result is correct.
    #[test]
    fn ut_udp_basic_v4() {
        block_on(async {
            let interface = Ipv4Addr::new(0, 0, 0, 0);
            let mut multi_addr = None;

            let socket_deal = |sender: &UdpSocket| {
                sender.set_ttl(101).unwrap();
                assert_eq!(sender.ttl().unwrap(), 101);
                assert!(sender.take_error().unwrap().is_none());
                sender.set_multicast_loop_v4(false).unwrap();
                assert!(!sender.multicast_loop_v4().unwrap());
                sender.set_multicast_ttl_v4(42).unwrap();
                assert_eq!(sender.multicast_ttl_v4().unwrap(), 42);

                for i in 0..255 {
                    let addr = Ipv4Addr::new(224, 0, 0, i);
                    if sender.join_multicast_v4(&addr, &interface).is_ok() {
                        multi_addr = Some(addr);
                        break;
                    }
                }

                if let Some(addr) = multi_addr {
                    sender
                        .leave_multicast_v4(&addr, &interface)
                        .expect("Cannot leave the multicast group");
                }
            };

            let (connected_sender, _) = udp_try_bind_connect(ADDR, socket_deal).await.unwrap();

            connected_sender.set_ttl(101).unwrap();
            assert_eq!(connected_sender.ttl().unwrap(), 101);
            assert!(connected_sender.take_error().unwrap().is_none());
            connected_sender.set_multicast_loop_v4(false).unwrap();
            assert!(!connected_sender.multicast_loop_v4().unwrap());
            connected_sender.set_multicast_ttl_v4(42).unwrap();
            assert_eq!(connected_sender.multicast_ttl_v4().unwrap(), 42);

            if let Some(addr) = multi_addr {
                connected_sender
                    .join_multicast_v4(&addr, &interface)
                    .expect("Cannot join the multicast group");
                connected_sender
                    .leave_multicast_v4(&multi_addr.unwrap(), &interface)
                    .expect("Cannot leave the multicast group");
            }
        });
    }

    /// Basic UT test cases for `UdpSocket` with `SocketAddrV6`.
    ///
    /// # Brief
    /// 1. Bind and connect `UdpSocket`.
    /// 2. Call set_multicast_loop_v6(), multicast_loop_v6() for `UdpSocket` and
    ///    `ConnectedUdpSocket`.
    /// 3. Check result is correct.
    #[test]
    fn ut_udp_basic_v6() {
        block_on(async {
            let addr = "::1:0";

            let interface = 1_u32;
            let mut multi_addr = None;
            let socket_deal = |sender: &UdpSocket| {
                sender.set_multicast_loop_v6(false).unwrap();
                assert!(!sender.multicast_loop_v6().unwrap());

                for i in 10..0xFFFF {
                    let addr = Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, i);
                    if sender.join_multicast_v6(&addr, interface).is_ok() {
                        multi_addr = Some(addr);
                        break;
                    }
                }

                if let Some(addr) = multi_addr {
                    sender
                        .leave_multicast_v6(&addr, interface)
                        .expect("Cannot leave the multicast group");
                }
            };
            let (connected_sender, _) = udp_try_bind_connect(addr, socket_deal).await.unwrap();

            connected_sender.set_multicast_loop_v6(false).unwrap();
            assert!(!connected_sender.multicast_loop_v6().unwrap());

            if let Some(addr) = multi_addr {
                connected_sender
                    .join_multicast_v6(&addr, interface)
                    .expect("Cannot join the multicast group");
                connected_sender
                    .leave_multicast_v6(&addr, interface)
                    .expect("Cannot leave the multicast group");
            }
        });
    }

    /// UT test cases for `poll_send()` and `poll_recv()`.
    ///
    /// # Brief
    /// 1. Create UdpSocket and connect to the remote address.
    /// 2. Sender calls poll_fn() to send message first.
    /// 3. Receiver calls poll_fn() to receive message.
    /// 4. Check if the test results are correct.
    #[test]
    fn ut_send_recv_poll() {
        let handle = spawn(async move {
            let (connected_sender, connected_receiver) =
                udp_try_bind_connect(ADDR, |_| {}).await.unwrap();
            let n = poll_fn(|cx| connected_sender.poll_send(cx, b"Hello"))
                .await
                .expect("Sender Send Failed");
            assert_eq!(n, "Hello".len());

            let mut recv_buf = [0_u8; 12];
            let mut read = ReadBuf::new(&mut recv_buf);
            poll_fn(|cx| connected_receiver.poll_recv(cx, &mut read))
                .await
                .unwrap();

            assert_eq!(read.filled(), b"Hello");
        });
        block_on(handle).expect("block_on failed");
    }

    /// UT test cases for `poll_send_to()` and `poll_recv_from()`.
    ///
    /// # Brief
    /// 1. Create UdpSocket.
    /// 2. Sender calls poll_fn() to send message to the specified address.
    /// 3. Receiver calls poll_fn() to receive message and return the address
    ///    the message from.
    /// 4. Check if the test results are correct.
    #[test]
    fn ut_send_to_recv_from_poll() {
        let handle = spawn(async move {
            let sender = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
            let receiver = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");

            let sender_addr = sender.local_addr().unwrap();
            let receiver_addr = receiver.local_addr().unwrap();
            let n = poll_fn(|cx| sender.poll_send_to(cx, b"Hello", receiver_addr))
                .await
                .expect("Sender Send Failed");
            assert_eq!(n, "Hello".len());

            let mut recv_buf = [0_u8; 12];
            let mut read = ReadBuf::new(&mut recv_buf);
            let addr = poll_fn(|cx| receiver.poll_recv_from(cx, &mut read))
                .await
                .unwrap();
            assert_eq!(read.filled(), b"Hello");
            assert_eq!(addr, sender_addr);
        });
        block_on(handle).expect("block_on failed");
    }

    /// UT test cases for `broadcast()` and `set_broadcast()`.
    ///
    /// # Brief
    /// 1. Create UdpSocket.
    /// 2. Sender calls set_broadcast() to set broadcast.
    /// 3. Sender calls broadcast() to get broadcast.
    /// 4. Check if the test results are correct.
    #[test]
    fn ut_set_get_broadcast() {
        let handle = spawn(async move {
            let broadcast_socket = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
            broadcast_socket
                .set_broadcast(true)
                .expect("set_broadcast failed");

            assert!(broadcast_socket.broadcast().expect("get broadcast failed"));
        });
        block_on(handle).expect("block_on failed");

        let handle = spawn(async move {
            let (broadcast_socket, _) = udp_try_bind_connect(ADDR, |_| {}).await.unwrap();
            broadcast_socket
                .set_broadcast(true)
                .expect("set_broadcast failed");

            assert!(broadcast_socket.broadcast().expect("get broadcast failed"));
        });
        block_on(handle).expect("block_on failed");
    }

    /// UT test cases for `local_addr()`.
    ///
    /// # Brief
    /// 1. Create UdpSocket.
    /// 2. Sender calls local_addr() to get local address.
    /// 3. Check if the test results are correct.
    #[test]
    fn ut_get_local_addr() {
        let handle = spawn(async move {
            let mut local_addr = None;
            let socket_deal = |socket: &UdpSocket| local_addr = Some(socket.local_addr().unwrap());
            let (connected_sock, _) = udp_try_bind_connect(ADDR, socket_deal).await.unwrap();

            let connect_local_addr = connected_sock.local_addr().expect("local_addr failed");
            assert_eq!(connect_local_addr, local_addr.unwrap());
        });
        block_on(handle).expect("block_on failed");
    }

    /// UT test cases for `peer_addr()`.
    ///
    /// # Brief
    /// 1. Create UdpSocket.
    /// 2. Sender calls peer_addr() to get the socket address of the remote
    ///    peer.
    /// 3. Check if the test results are correct.
    #[test]
    fn ut_get_peer_addr() {
        let handle = spawn(async move {
            let mut local_addr = None;
            let socket_deal = |socket: &UdpSocket| local_addr = Some(socket.local_addr().unwrap());
            let (_, peer_socket) = udp_try_bind_connect(ADDR, socket_deal).await.unwrap();

            assert_eq!(
                peer_socket.peer_addr().expect("peer_addr failed"),
                local_addr.unwrap()
            );
        });
        block_on(handle).expect("block_on failed");
    }

    /// UT test cases for `send_to()` and `peek_from()`.
    ///
    /// # Brief
    /// 1. Create UdpSocket.
    /// 2. Sender calls send_to() to send message to the specified address.
    /// 3. Receiver calls peek_from() to receive message and return the number
    ///    of bytes peeked.
    /// 4. Check if the test results are correct.
    #[test]
    fn ut_send_to_peek_from() {
        let handle = spawn(async move {
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

        block_on(handle).expect("block_on failed");
    }

    /// UT test cases for `send_to()` and `try_peek_from()`.
    ///
    /// # Brief
    /// 1. Create UdpSocket.
    /// 2. Sender calls send_to() to send message to the specified address.
    /// 3. Receiver calls readable() to wait for the socket to become readable.
    /// 4. Receiver calls try_peek_from() to receive message and return the
    ///    number of bytes peeked.
    /// 5. Check if the test results are correct.
    #[test]
    fn ut_send_to_try_peek_from() {
        let handle = spawn(async move {
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

        block_on(handle).expect("block_on failed");
    }

    /// UT test cases for `poll_send_to()` and `poll_peek_from()`.
    ///
    /// # Brief
    /// 1. Create UdpSocket.
    /// 2. Sender calls poll_fn() to send message to the specified address.
    /// 3. Receiver calls poll_fn() to receive message and return the address
    ///    the message from.
    /// 4. Check if the test results are correct.
    #[test]
    fn ut_send_to_peek_from_poll() {
        let handle = spawn(async move {
            let sender = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
            let receiver = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");

            let sender_addr = sender.local_addr().unwrap();
            let receiver_addr = receiver.local_addr().unwrap();

            let n = poll_fn(|cx| sender.poll_send_to(cx, b"Hello", receiver_addr))
                .await
                .expect("Sender Send Failed");
            assert_eq!(n, "Hello".len());

            let mut recv_buf = [0_u8; 12];
            let mut read = ReadBuf::new(&mut recv_buf);
            let addr = poll_fn(|cx| receiver.poll_peek_from(cx, &mut read))
                .await
                .unwrap();
            assert_eq!(read.filled(), b"Hello");
            assert_eq!(addr, sender_addr);
        });
        block_on(handle).expect("block_on failed");
    }

    /// UT test cases for `peek()` in ConnectedUdpSocket.
    ///
    /// # Brief
    /// 1. Create UdpSocket.
    /// 2. Sender calls send_to() to send message to the specified address.
    /// 3. Receiver calls connect() to create a ConnectedUdpSocket.
    /// 4. ConnectedUdpSocket calls peek() to receive message.
    /// 5. Check if the test results are correct.
    #[test]
    fn ut_connected_peek() {
        let handle = spawn(async move {
            let sender = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
            let receiver = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");

            let sender_addr = sender.local_addr().unwrap();
            let receiver_addr = receiver.local_addr().unwrap();

            let connect_socket = receiver.connect(sender_addr).await.unwrap();

            let send_buf = [2; 6];
            sender
                .send_to(&send_buf, receiver_addr)
                .await
                .expect("Send data Failed");

            let mut buf = [0; 10];
            let number_of_bytes = connect_socket
                .peek(&mut buf)
                .await
                .expect("Didn't receive data");

            assert_eq!(number_of_bytes, 6);
        });

        block_on(handle).expect("block_on failed");
    }

    /// UT test cases for `try_peek()` in ConnectedUdpSocket.
    ///
    /// # Brief
    /// 1. Create UdpSocket.
    /// 2. Sender calls send_to() to send message to the specified address.
    /// 3. Receiver calls connect() to create a ConnectedUdpSocket.
    /// 4. ConnectedUdpSocket waits to be readable, then calls try_peek() to
    ///    receive message.
    /// 5. Check if the test results are correct.
    #[test]
    fn ut_connected_try_peek() {
        let handle = spawn(async move {
            let sender = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");
            let receiver = UdpSocket::bind(ADDR).await.expect("Bind Socket Failed");

            let sender_addr = sender.local_addr().unwrap();
            let receiver_addr = receiver.local_addr().unwrap();

            let connect_socket = receiver.connect(sender_addr).await.unwrap();

            let send_buf = [2; 6];
            sender
                .send_to(&send_buf, receiver_addr)
                .await
                .expect("Send data Failed");

            let mut buf = [0; 10];
            connect_socket.readable().await.unwrap();
            let number_of_bytes = connect_socket
                .try_peek(&mut buf)
                .expect("Didn't receive data");

            assert_eq!(number_of_bytes, 6);
        });

        block_on(handle).expect("block_on failed");
    }
}

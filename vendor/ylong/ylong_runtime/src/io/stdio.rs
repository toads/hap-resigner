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

use std::io::{Read, Write};
use std::{cmp, io};

use crate::io::ReadBuf;
use crate::task::JoinHandle;

const MAX_BUF: usize = 2 * 1024 * 1024;

pub(crate) enum State<T> {
    Idle(Option<BufInner>),
    Poll(JoinHandle<(io::Result<usize>, BufInner, T)>),
}

impl<T> State<T> {
    pub(crate) fn init() -> Self {
        State::Idle(Some(BufInner::new()))
    }
}

pub(crate) struct BufInner {
    inner: Vec<u8>,
    pos: usize,
}

impl BufInner {
    fn new() -> Self {
        BufInner {
            inner: Vec::with_capacity(0),
            pos: 0,
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub(crate) fn len(&self) -> usize {
        self.inner.len() - self.pos
    }

    fn bytes(&self) -> &[u8] {
        &self.inner[self.pos..]
    }

    pub(crate) fn set_len(&mut self, buf: &mut ReadBuf<'_>) {
        let len = cmp::min(buf.remaining(), MAX_BUF);
        if self.inner.len() < len {
            self.inner.reserve(len - self.len());
        }
        unsafe {
            self.inner.set_len(len);
        }
    }

    pub(crate) fn clone_from(&mut self, buf: &[u8]) -> usize {
        let n = cmp::min(buf.len(), MAX_BUF);
        self.inner.extend_from_slice(&buf[..n]);
        n
    }

    pub(crate) fn clone_into(&mut self, buf: &mut ReadBuf<'_>) -> usize {
        let n = cmp::min(self.len(), buf.remaining());
        buf.append(&self.bytes()[..n]);
        self.pos += n;

        if self.pos == self.inner.len() {
            self.inner.truncate(0);
            self.pos = 0;
        }
        n
    }

    pub(crate) fn read_from<T: Read>(&mut self, std: &mut T) -> io::Result<usize> {
        let res = loop {
            match std.read(&mut self.inner) {
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
                res => break res,
            }
        };

        match res {
            Ok(n) => self.inner.truncate(n),
            Err(_) => self.inner.clear(),
        }

        res
    }

    pub(crate) fn write_into<T: Write>(&mut self, std: &mut T) -> io::Result<()> {
        let res = std.write_all(&self.inner);
        self.inner.clear();
        res
    }
}

macro_rules! std_async_write {
    () => {
        fn poll_write(
            mut self: Pin<&mut Self>,
            cx: &mut Context<'_>,
            buf: &[u8],
        ) -> Poll<io::Result<usize>> {
            loop {
                match self.state {
                    State::Idle(ref mut buf_op) => {
                        let mut buf_inner = buf_op.take().unwrap();

                        if !buf_inner.is_empty() {
                            return Poll::Ready(Err(io::Error::new(
                                io::ErrorKind::AlreadyExists,
                                "inner Buf must be empty before poll!",
                            )));
                        }

                        let n = buf_inner.clone_from(buf);

                        let mut std = self.std.take().unwrap();

                        let handle = spawn_blocking(move || {
                            let res = buf_inner.write_into(&mut std).map(|_| n);

                            (res, buf_inner, std)
                        });

                        self.state = State::Poll(handle);
                        self.has_written = true;
                    }
                    State::Poll(ref mut join_handle) => {
                        let (res, buf_inner, std) = match Pin::new(join_handle).poll(cx)? {
                            Poll::Ready(t) => t,
                            Poll::Pending => return Poll::Pending,
                        };
                        self.state = State::Idle(Some(buf_inner));
                        self.std = Some(std);

                        let n = res?;
                        return Poll::Ready(Ok(n));
                    }
                }
            }
        }

        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            loop {
                let has_written = self.has_written;
                match self.state {
                    State::Idle(ref mut buf_cell) => {
                        if !has_written {
                            return Poll::Ready(Ok(()));
                        }
                        let buf = buf_cell.take().unwrap();
                        let mut inner = self.std.take().unwrap();

                        self.state = State::Poll(spawn_blocking(move || {
                            let res = inner.flush().map(|_| 0);
                            (res, buf, inner)
                        }));

                        self.has_written = false;
                    }
                    State::Poll(ref mut join_handle) => {
                        let (res, buf, std) = match Pin::new(join_handle).poll(cx)? {
                            Poll::Ready(t) => t,
                            Poll::Pending => return Poll::Pending,
                        };
                        self.state = State::Idle(Some(buf));
                        self.std = Some(std);

                        res?;
                    }
                }
            }
        }

        fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    };
}
pub(crate) use std_async_write;

#[cfg(test)]
mod test {
    use crate::io::stdio::BufInner;
    use crate::io::ReadBuf;

    /// UT test cases for `stdout` and `stderr``.
    ///
    /// # Brief
    /// 1. create a `stdout` and a `stderr`.
    /// 2. write something into `stdout` and `stderr`.
    /// 3. check operation is ok.
    #[test]
    fn ut_test_stdio_basic() {
        let mut buf_inner = BufInner::new();
        assert_eq!(buf_inner.pos, 0);
        assert!(buf_inner.inner.is_empty());
        assert!(buf_inner.is_empty());

        let mut buf = [1; 10];
        let mut read_buf = ReadBuf::new(&mut buf);
        buf_inner.set_len(&mut read_buf);
        assert_eq!(buf_inner.len(), 10);

        let mut buf = [0; 20];
        let mut read_buf = ReadBuf::new(&mut buf);
        let n = buf_inner.clone_into(&mut read_buf);
        assert_eq!(n, 10);
    }
}

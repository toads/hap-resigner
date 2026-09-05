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
use std::mem::MaybeUninit;
use std::pin::Pin;
use std::slice::from_raw_parts_mut;
use std::string::FromUtf8Error;
use std::task::{Context, Poll};
use std::{io, mem};

use crate::futures::poll_fn;
use crate::io::async_buf_read::AsyncBufRead;
use crate::io::async_read::AsyncRead;
use crate::io::poll_ready;
use crate::io::read_buf::ReadBuf;

macro_rules! take_reader {
    ($self: expr) => {
        match $self.reader.take() {
            Some(reader) => reader,
            None => panic!("read: poll after finished"),
        }
    };
}

/// A future for reading available data from the source into a buffer.
///
/// Returned by [`crate::io::AsyncReadExt::read`]
pub struct ReadTask<'a, R: ?Sized> {
    reader: Option<&'a mut R>,
    buf: &'a mut [u8],
}

impl<'a, R: ?Sized> ReadTask<'a, R> {
    #[inline(always)]
    pub(crate) fn new(reader: &'a mut R, buf: &'a mut [u8]) -> ReadTask<'a, R> {
        ReadTask {
            reader: Some(reader),
            buf,
        }
    }
}

impl<'a, R> Future for ReadTask<'a, R>
where
    R: AsyncRead + Unpin,
{
    type Output = io::Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut reader = take_reader!(self);

        let mut buf = ReadBuf::new(self.buf);
        match Pin::new(&mut reader).poll_read(cx, &mut buf) {
            Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
            Poll::Ready(_) => Poll::Ready(Ok(buf.filled_len())),
            Poll::Pending => {
                self.reader = Some(reader);
                Poll::Pending
            }
        }
    }
}

/// A future for reading every data from the source into a vector.
///
/// Returned by [`crate::io::AsyncReadExt::read_to_end`]
pub struct ReadToEndTask<'a, R: ?Sized> {
    reader: &'a mut R,
    buf: &'a mut Vec<u8>,
    r_len: usize,
}

impl<'a, R: ?Sized> ReadToEndTask<'a, R> {
    #[inline(always)]
    pub(crate) fn new(reader: &'a mut R, buf: &'a mut Vec<u8>) -> ReadToEndTask<'a, R> {
        ReadToEndTask {
            reader,
            buf,
            r_len: 0,
        }
    }
}
const PROBE_SIZE: usize = 32;

fn poll_read_to_end<R: AsyncRead + Unpin>(
    buf: &mut Vec<u8>,
    mut reader: &mut R,
    read_len: &mut usize,
    cx: &mut Context<'_>,
) -> Poll<io::Result<usize>> {
    loop {
        // Allocate spaces to read, if the remaining capacity is larger than 32
        // bytes, this will do nothing.
        buf.try_reserve(PROBE_SIZE)
            .map_err(|_| io::ErrorKind::OutOfMemory)?;
        let len = buf.len();
        let mut read_buf = ReadBuf::uninit(unsafe {
            from_raw_parts_mut(buf.as_mut_ptr().cast::<MaybeUninit<u8>>(), buf.capacity())
        });
        read_buf.assume_init(len);
        read_buf.set_filled(len);

        let poll = Pin::new(&mut reader).poll_read(cx, &mut read_buf);
        let new_len = read_buf.filled_len();
        match poll {
            Poll::Pending => return Poll::Pending,
            Poll::Ready(Ok(())) if (new_len - len) == 0 => {
                return Poll::Ready(Ok(mem::replace(read_len, 0)))
            }
            Poll::Ready(Ok(())) => {
                *read_len += new_len - len;
                unsafe {
                    buf.set_len(new_len);
                }
            }
            Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
        }
    }
}

impl<'a, R> Future for ReadToEndTask<'a, R>
where
    R: AsyncRead + Unpin,
{
    type Output = io::Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let me = self.get_mut();
        let (buf, reader, read_len) = (&mut me.buf, &mut me.reader, &mut me.r_len);
        poll_read_to_end(buf, *reader, read_len, cx)
    }
}

/// A future for reading every data from the source into a String.
///
/// Returned by [`crate::io::AsyncReadExt::read_to_string`]
pub struct ReadToStringTask<'a, R: ?Sized> {
    reader: &'a mut R,
    buf: Vec<u8>,
    output: &'a mut String,
    r_len: usize,
}

impl<'a, R: ?Sized> ReadToStringTask<'a, R> {
    #[inline(always)]
    pub(crate) fn new(reader: &'a mut R, dst: &'a mut String) -> ReadToStringTask<'a, R> {
        ReadToStringTask {
            reader,
            buf: mem::take(dst).into_bytes(),
            output: dst,
            r_len: 0,
        }
    }
}

fn io_string_result(
    io_res: io::Result<usize>,
    str_res: Result<String, FromUtf8Error>,
    read_len: usize,
    output: &mut String,
) -> Poll<io::Result<usize>> {
    match (io_res, str_res) {
        (Ok(bytes), Ok(string)) => {
            *output = string;
            Poll::Ready(Ok(bytes))
        }
        (Ok(bytes), Err(trans_err)) => {
            let mut vector = trans_err.into_bytes();
            let len = vector.len() - bytes;
            vector.truncate(len);
            *output = String::from_utf8(vector)
                .unwrap_or_else(|e| panic!("Invalid utf-8 data, error: {e}"));
            Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Invalid utf-8 data",
            )))
        }
        (Err(io_err), Ok(string)) => {
            *output = string;
            Poll::Ready(Err(io_err))
        }
        (Err(io_err), Err(trans_err)) => {
            let mut vector = trans_err.into_bytes();
            let len = vector.len() - read_len;
            vector.truncate(len);
            *output = String::from_utf8(vector)
                .unwrap_or_else(|e| panic!("Invalid utf-8 data, error: {e}"));
            Poll::Ready(Err(io_err))
        }
    }
}

impl<'a, R> Future for ReadToStringTask<'a, R>
where
    R: AsyncRead + Unpin,
{
    type Output = io::Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let me = self.get_mut();
        let (buf, output, reader, read_len) =
            (&mut me.buf, &mut me.output, &mut me.reader, &mut me.r_len);
        let res = poll_ready!(poll_read_to_end(buf, *reader, read_len, cx));
        let trans = String::from_utf8(mem::take(buf));

        io_string_result(res, trans, *read_len, output)
    }
}

/// A future for reading exact amount of bytes from the source into a vector.
///
/// Returned by [`crate::io::AsyncReadExt::read_exact`]
pub struct ReadExactTask<'a, R: ?Sized> {
    reader: Option<&'a mut R>,
    buf: ReadBuf<'a>,
}

impl<'a, R: ?Sized> ReadExactTask<'a, R> {
    #[inline(always)]
    pub(crate) fn new(reader: &'a mut R, buf: &'a mut [u8]) -> ReadExactTask<'a, R> {
        ReadExactTask {
            reader: Some(reader),
            buf: ReadBuf::new(buf),
        }
    }
}

impl<'a, R> Future for ReadExactTask<'a, R>
where
    R: AsyncRead + Unpin,
{
    type Output = io::Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut reader = take_reader!(self);
        let this = self.get_mut();

        loop {
            let remain = this.buf.remaining();
            if remain == 0 {
                return Poll::Ready(Ok(()));
            }
            let _ = match Pin::new(&mut reader).poll_read(cx, &mut this.buf) {
                Poll::Pending => {
                    this.reader = Some(reader);
                    return Poll::Pending;
                }
                x => x?,
            };
            if this.buf.remaining() == remain {
                return Poll::Ready(Err(io::ErrorKind::UnexpectedEof.into()));
            }
        }
    }
}

/// A future for reading every data from the source into a vector until the
/// desired delimiter appears.
///
/// Returned by [`crate::io::AsyncBufReadExt::read_until`]
pub struct ReadUtilTask<'a, R: ?Sized> {
    reader: &'a mut R,
    r_len: usize,
    delim: u8,
    buf: &'a mut Vec<u8>,
}

impl<'a, R: ?Sized> ReadUtilTask<'a, R> {
    #[inline(always)]
    pub(crate) fn new(reader: &'a mut R, delim: u8, buf: &'a mut Vec<u8>) -> ReadUtilTask<'a, R> {
        ReadUtilTask {
            reader,
            r_len: 0,
            delim,
            buf,
        }
    }
}

fn poll_read_until<R: AsyncBufRead + Unpin>(
    buf: &mut Vec<u8>,
    mut reader: &mut R,
    delim: u8,
    read_len: &mut usize,
    cx: &mut Context<'_>,
) -> Poll<io::Result<usize>> {
    loop {
        let (done, used) = {
            let available = poll_ready!(Pin::new(&mut reader).poll_fill_buf(cx))?;

            let ret = available.iter().position(|&val| val == delim);

            match ret {
                None => {
                    buf.extend_from_slice(available);
                    (false, available.len())
                }
                Some(i) => {
                    buf.extend_from_slice(&available[..=i]);
                    (true, i + 1)
                }
            }
        };
        Pin::new(&mut reader).consume(used);
        *read_len += used;
        if done || used == 0 {
            return Poll::Ready(Ok(mem::replace(read_len, 0)));
        }
    }
}

impl<'a, R> Future for ReadUtilTask<'a, R>
where
    R: AsyncBufRead + Unpin,
{
    type Output = io::Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let me = self.get_mut();
        let (buf, reader, delim, read_len) = (&mut me.buf, &mut me.reader, me.delim, &mut me.r_len);
        poll_read_until(buf, *reader, delim, read_len, cx)
    }
}

/// A future for reading every data from the source into a vector until the
/// desired delimiter appears.
///
/// Returned by [`crate::io::AsyncBufReadExt::read_until`]
pub struct ReadLineTask<'a, R: ?Sized> {
    reader: &'a mut R,
    r_len: usize,
    buf: Vec<u8>,
    output: &'a mut String,
}

impl<'a, R: ?Sized> ReadLineTask<'a, R> {
    #[inline(always)]
    pub(crate) fn new(reader: &'a mut R, buf: &'a mut String) -> ReadLineTask<'a, R> {
        ReadLineTask {
            reader,
            r_len: 0,
            buf: mem::take(buf).into_bytes(),
            output: buf,
        }
    }
}

impl<'a, R> Future for ReadLineTask<'a, R>
where
    R: AsyncBufRead + Unpin,
{
    type Output = io::Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let me = self.get_mut();
        let (buf, output, reader, read_len) =
            (&mut me.buf, &mut me.output, &mut me.reader, &mut me.r_len);
        let res = poll_ready!(poll_read_until(buf, *reader, b'\n', read_len, cx));
        let trans = String::from_utf8(mem::take(buf));

        io_string_result(res, trans, *read_len, output)
    }
}

/// A future for reading every data from the source into a vector and splitting
/// it into segments by a delimiter.
///
/// Returned by [`crate::io::AsyncBufReadExt::split`]
pub struct SplitTask<R> {
    reader: R,
    delim: u8,
    buf: Vec<u8>,
    r_len: usize,
}

impl<R> SplitTask<R>
where
    R: AsyncBufRead + Unpin,
{
    pub(crate) fn new(reader: R, delim: u8) -> SplitTask<R> {
        SplitTask {
            reader,
            delim,
            buf: Vec::new(),
            r_len: 0,
        }
    }

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<Option<Vec<u8>>>> {
        let me = self.get_mut();
        let (buf, reader, read_len, delim) = (&mut me.buf, &mut me.reader, &mut me.r_len, me.delim);
        let res = poll_ready!(poll_read_until(buf, reader, delim, read_len, cx))?;

        if buf.is_empty() && res == 0 {
            return Poll::Ready(Ok(None));
        }

        if buf.last() == Some(&delim) {
            buf.pop();
        }
        Poll::Ready(Ok(Some(mem::take(buf))))
    }

    pub async fn next(&mut self) -> io::Result<Option<Vec<u8>>> {
        poll_fn(|cx| Pin::new(&mut *self).poll_next(cx)).await
    }
}

/// A future for reading every data from the source into a vector and splitting
/// it into segments by row.
///
/// Returned by [`crate::io::AsyncBufReadExt::split`]
pub struct LinesTask<R> {
    reader: R,
    buf: Vec<u8>,
    output: String,
    r_len: usize,
}

impl<R> LinesTask<R>
where
    R: AsyncBufRead,
{
    pub(crate) fn new(reader: R) -> LinesTask<R> {
        LinesTask {
            reader,
            buf: Vec::new(),
            output: String::new(),
            r_len: 0,
        }
    }
}

impl<R> LinesTask<R>
where
    R: AsyncBufRead + Unpin,
{
    fn poll_next_line(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<Option<String>>> {
        let me = self.get_mut();
        let (buf, output, reader, read_len) =
            (&mut me.buf, &mut me.output, &mut me.reader, &mut me.r_len);
        let io_res = poll_ready!(poll_read_until(buf, reader, b'\n', read_len, cx));
        let str_res = String::from_utf8(mem::take(buf));

        let res = poll_ready!(io_string_result(io_res, str_res, *read_len, output))?;

        if output.is_empty() && res == 0 {
            return Poll::Ready(Ok(None));
        }

        if output.ends_with('\n') {
            output.pop();
            if output.ends_with('\r') {
                output.pop();
            }
        }
        Poll::Ready(Ok(Some(mem::take(output))))
    }

    pub async fn next_line(&mut self) -> io::Result<Option<String>> {
        poll_fn(|cx| Pin::new(&mut *self).poll_next_line(cx)).await
    }
}

#[cfg(all(test, feature = "fs"))]
mod test {
    use crate::fs::{remove_file, File};
    use crate::io::async_read::AsyncReadExt;
    use crate::io::async_write::AsyncWriteExt;
    use crate::io::AsyncBufReader;

    /// UT test cases for `io_string_result()`.
    ///
    /// # Brief
    /// 1. Create a file and write non-utf8 chars to it.
    /// 2. Create a AsyncBufReader.
    /// 3. Call io_string_result() to translate the content of the file to
    ///    String.
    /// 4. Check if the test results are expected errors.
    #[test]
    fn ut_io_string_result() {
        let handle = crate::spawn(async move {
            let file_path = "foo.txt";

            let mut f = File::create(file_path).await.unwrap();
            let buf = [0, 159, 146, 150];
            let n = f.write(&buf).await.unwrap();
            assert_eq!(n, 4);

            let f = File::open(file_path).await.unwrap();
            let mut reader = AsyncBufReader::new(f);
            let mut buf = String::new();
            let res = reader.read_to_string(&mut buf).await;
            assert!(res.is_err());
            assert_eq!(res.unwrap_err().kind(), std::io::ErrorKind::InvalidData);

            let res = remove_file(file_path).await;
            assert!(res.is_ok());
        });
        crate::block_on(handle).expect("failed to block on");
    }
}

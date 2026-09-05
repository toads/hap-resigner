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

use std::io;
use std::io::SeekFrom;
use std::ops::DerefMut;
use std::pin::Pin;
use std::task::{Context, Poll};

use crate::io::seek_task::SeekTask;

/// An asynchronous version of [`std::io::Seek`].
///
/// The `AsyncSeek` trait provides a cursor which can be moved within a stream
/// of bytes asynchronously.
///
/// The stream typically has a fixed size, allowing seeking relative to either
/// end or the current offset.
pub trait AsyncSeek {
    /// Attempts to seek to a position in an I/O source.
    ///
    /// If succeeds, this method will return `Poll::Ready(Ok(n))` where `n`
    /// indicates the current position in the I/O source.
    ///
    /// If `Poll::Pending` is returned, it means that the input source is
    /// currently not ready for seeking. In this case, this task will be put
    /// to sleep until the underlying stream becomes readable or closed.
    fn poll_seek(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        pos: SeekFrom,
    ) -> Poll<io::Result<u64>>;
}

impl<T> AsyncSeek for Pin<T>
where
    T: DerefMut<Target = dyn AsyncSeek> + Unpin,
{
    fn poll_seek(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        pos: SeekFrom,
    ) -> Poll<io::Result<u64>> {
        self.get_mut().as_mut().poll_seek(cx, pos)
    }
}

impl<T: AsyncSeek + Unpin + ?Sized> AsyncSeek for Box<T> {
    fn poll_seek(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        pos: SeekFrom,
    ) -> Poll<io::Result<u64>> {
        Pin::new(&mut **self).poll_seek(cx, pos)
    }
}

impl<T: AsyncSeek + Unpin + ?Sized> AsyncSeek for &mut T {
    fn poll_seek(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        pos: SeekFrom,
    ) -> Poll<io::Result<u64>> {
        Pin::new(&mut **self).poll_seek(cx, pos)
    }
}

impl<T: AsyncSeek + ?Sized> AsyncSeekExt for T {}

/// An external trait that is automatically implemented for any object that has
/// the AsyncSeek trait. Provides std-like `seek` method.
/// `Seek` method in this trait returns a future object. Awaits on the future
/// will complete the task, but it doesn't guarantee whether the task will
/// finished immediately or asynchronously.
pub trait AsyncSeekExt: AsyncSeek {
    /// Asynchronously seek to an offset, in bytes, in a stream.
    ///
    /// A seek beyond the end of a stream is allowed, but the behavior is
    /// defined by the implementation.
    ///
    /// If the seek operation completed successfully,
    /// this method returns the new position from the start of the stream.
    ///
    /// # Errors
    ///
    /// Seeking can fail, for example because it might involve flushing a
    /// buffer.
    ///
    /// Seeking to a negative offset is considered an error.
    fn seek(&mut self, pos: SeekFrom) -> SeekTask<Self>
    where
        Self: Unpin,
    {
        SeekTask::new(self, pos)
    }

    /// Rewinds to the beginning of a stream.
    ///
    /// This is a convenience method, equivalent to seek(SeekFrom::Start(0)).
    ///
    /// # Errors
    ///
    /// Rewinding can fail, for example because it might involve flushing a
    /// buffer.
    ///
    /// # Example
    ///
    /// ```no run
    /// use std::io;
    ///
    /// use ylong_runtime::fs::OpenOptions;
    /// use ylong_runtime::io::AsyncSeekExt;
    ///
    /// async fn async_io() -> io::Result<()> {
    ///     let mut f = OpenOptions::new()
    ///         .write(true)
    ///         .read(true)
    ///         .create(true)
    ///         .open("foo.txt")
    ///         .await
    ///         .unwrap();
    ///
    ///     let hello = "Hello!\n";
    ///     f.rewind().await.unwrap();
    ///     Ok(())
    /// }
    /// ```
    fn rewind(&mut self) -> SeekTask<'_, Self>
    where
        Self: Unpin,
    {
        self.seek(SeekFrom::Start(0))
    }

    /// Returns the current seek position from the start of the stream.
    ///
    /// This is a convenience method, equivalent to
    /// `self.seek(SeekFrom::Current(0))`.
    ///
    /// # Example
    ///
    /// ```no run
    /// use std::io;
    ///
    /// use ylong_runtime::fs::{File, OpenOptions};
    /// use ylong_runtime::io::{AsyncBufReadExt, AsyncBufReader, AsyncSeekExt};
    ///
    /// async fn async_io() -> io::Result<()> {
    ///     let mut f = AsyncBufReader::new(File::open("foo.txt").await?);
    ///
    ///     let before = f.stream_position().await?;
    ///     f.read_line(&mut String::new()).await?;
    ///     let after = f.stream_position().await?;
    ///
    ///     println!("The first line was {} bytes long", after - before);
    ///     Ok(())
    /// }
    /// ```
    fn stream_position(&mut self) -> SeekTask<'_, Self>
    where
        Self: Unpin,
    {
        self.seek(SeekFrom::Current(0))
    }
}

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
use std::fs::{File as SyncFile, Metadata, Permissions};
use std::future::Future;
use std::io;
use std::io::{Seek, SeekFrom};
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use crate::fs::file_buf::FileBuf;
use crate::fs::{async_op, poll_ready};
use crate::futures::poll_fn;
use crate::io::{AsyncRead, AsyncSeek, AsyncWrite, ReadBuf};
use crate::spawn::spawn_blocking;
use crate::sync::Mutex;
use crate::task::{JoinHandle, TaskBuilder};

/// An asynchronous wrapping of [`std::fs::File`]. Provides async read/write
/// methods.
pub struct File {
    file: Arc<SyncFile>,
    inner: Mutex<FileInner>,
    // controls how many bytes the buffer could reserve during read
    // and how many bytes the buffer could extend per time during write
    buf_size_limit: usize,
}

struct FileInner {
    state: FileState,
    write_err: Option<io::ErrorKind>,
    idx: u64,
}

type RWJoinHandle = JoinHandle<(FileBuf, io::Result<()>)>;

type SeekJoinHandle = JoinHandle<(FileBuf, io::Result<u64>)>;

enum FileState {
    Idle(Option<FileBuf>),
    Reading(RWJoinHandle),
    Writing(RWJoinHandle),
    Seeking(SeekJoinHandle),
}

enum FileOp {
    Reading,
    Writing,
    Seeking,
}

impl Future for FileState {
    type Output = io::Result<u64>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let state = self.get_mut();
        match state {
            FileState::Idle(_) => unreachable!(),
            FileState::Reading(x) | FileState::Writing(x) => {
                let (file_buf, res) = poll_ready!(Pin::new(x).poll(cx))?;
                *state = FileState::Idle(Some(file_buf));
                // For read and write, we dont care about the output
                Poll::Ready(res.map(|_| 0_u64))
            }
            FileState::Seeking(x) => {
                let (file_buf, res) = poll_ready!(Pin::new(x).poll(cx))?;
                *state = FileState::Idle(Some(file_buf));
                Poll::Ready(res)
            }
        }
    }
}

impl FileState {
    #[inline]
    fn get_op(&self) -> FileOp {
        match self {
            FileState::Idle(_) => unreachable!(),
            FileState::Reading(_) => FileOp::Reading,
            FileState::Writing(_) => FileOp::Writing,
            FileState::Seeking(_) => FileOp::Seeking,
        }
    }
}

const DEFAULT_BUF_LIMIT: usize = 2 * 1024 * 1024;

impl Debug for File {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("YlongFile").field(&self.file).finish()
    }
}

impl File {
    /// Creates a new [`File`] struct.
    pub fn new(file: SyncFile) -> File {
        File {
            file: Arc::new(file),
            inner: Mutex::new(FileInner {
                state: FileState::Idle(Some(FileBuf::with_capacity(0))),
                write_err: None,
                idx: 0,
            }),
            buf_size_limit: DEFAULT_BUF_LIMIT,
        }
    }

    /// Attempts to open a file in read-only mode asynchronously.
    ///
    /// See the [`super::OpenOptions::open`] method for more details.
    ///
    /// # Errors
    ///
    /// This function will return an error if `path` does not already exist.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::fs::File;
    ///
    /// async fn open() -> std::io::Result<()> {
    ///     let mut f = File::open("foo.txt").await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn open<P: AsRef<Path>>(path: P) -> io::Result<File> {
        let path = path.as_ref().to_owned();
        let file = async_op(|| SyncFile::open(path)).await?;
        Ok(File::new(file))
    }

    /// Opens a file in write-only mode asynchronously.
    ///
    /// This function will create a file if it does not exist
    /// and truncate it if it does.
    ///
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::fs::File;
    ///
    /// async fn create() -> std::io::Result<()> {
    ///     let mut f = File::create("foo.txt").await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn create<P: AsRef<Path>>(path: P) -> io::Result<File> {
        let path = path.as_ref().to_owned();
        let file = async_op(|| SyncFile::create(path)).await?;
        Ok(File::new(file))
    }

    /// Changes the permissions on the underlying file asynchronously.
    ///
    /// # Errors
    /// This function will return an error if the user lacks permission change
    /// attributes on the underlying file. It may also return an error in other
    /// os-specific unspecified cases.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::fs::File;
    ///
    /// async fn set_permissions() -> std::io::Result<()> {
    ///     let file = File::open("foo.txt").await?;
    ///     let mut perms = file.metadata().await?.permissions();
    ///     perms.set_readonly(true);
    ///     file.set_permissions(perms).await?;
    ///     Ok(())
    /// }
    /// ```
    ///
    /// Note that this method alters the permissions of the underlying file,
    /// even though it takes `&self` rather than `&mut self`.
    pub async fn set_permissions(&self, perm: Permissions) -> io::Result<()> {
        let file = self.file.clone();
        async_op(move || file.set_permissions(perm)).await
    }

    /// Attempts to sync all OS-internal metadata to disk asynchronously.
    ///
    /// This function will attempt to ensure that all in-memory data reaches the
    /// filesystem before returning.
    ///
    /// This can be used to handle errors that would otherwise only be caught
    /// when the `File` is closed. Dropping a file will ignore errors in
    /// synchronizing this in-memory data.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::fs::File;
    /// use ylong_runtime::io::AsyncWriteExt;
    ///
    /// async fn sync_all() -> std::io::Result<()> {
    ///     let mut f = File::create("foo.txt").await?;
    ///     f.write_all(b"Hello, world!").await?;
    ///
    ///     f.sync_all().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn sync_all(&self) -> io::Result<()> {
        let mut file = self.inner.lock().await;
        file.flush().await;
        let file = self.file.clone();
        async_op(move || file.sync_all()).await
    }

    /// This function is similar to [`File::sync_all`], except that it might not
    /// synchronize file metadata to the filesystem.
    ///
    /// This is intended for use cases that must synchronize content, but don't
    /// need the metadata on disk. The goal of this method is to reduce disk
    /// operations.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::fs::File;
    /// use ylong_runtime::io::AsyncWriteExt;
    ///
    /// async fn sync_data() -> std::io::Result<()> {
    ///     let mut f = File::create("foo.txt").await?;
    ///     f.write_all(b"Hello, world!").await?;
    ///
    ///     f.sync_data().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn sync_data(&self) -> io::Result<()> {
        let mut file = self.inner.lock().await;
        file.flush().await;
        let file = self.file.clone();
        async_op(move || file.sync_data()).await
    }

    /// Truncates or extends the underlying file, updating the size of this file
    /// to become size. If the size is less than the current file's size,
    /// then the file will be shrunk. If it is greater than the current file's
    /// size, then the file will be extended to size and have all of the
    /// intermediate data filled in with 0s. The file's cursor isn't
    /// changed. In particular, if the cursor was at the end and the file is
    /// shrunk using this operation, the cursor will now be past the end.
    ///
    /// # Errors
    ///
    /// This function will return an error if the file is not opened for
    /// writing.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::fs::File;
    ///
    /// async fn set_len() -> std::io::Result<()> {
    ///     let mut f = File::create("foo.txt").await?;
    ///     f.set_len(10).await?;
    ///
    ///     Ok(())
    /// }
    /// ```
    pub async fn set_len(&self, size: u64) -> io::Result<()> {
        let mut file = self.inner.lock().await;
        file.flush().await;

        let mut buf = match file.state {
            // after each take, buf will be set right back
            FileState::Idle(ref mut buf) => buf.take().unwrap(),
            _ => unreachable!(),
        };

        let arc_file = self.file.clone();

        let (buf, res) = spawn_blocking(&TaskBuilder::new(), move || {
            let res = if buf.remaining() == 0 {
                (&*arc_file)
                    .seek(SeekFrom::Current(buf.drop_unread()))
                    .and_then(|_| arc_file.set_len(size))
            } else {
                arc_file.set_len(size)
            }
            .map(|_| 0);

            (buf, res)
        })
        .await?;

        file.state = FileState::Idle(Some(buf));

        res.map(|u| file.idx = u)
    }

    /// Queries metadata about the underlying file asynchronously.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::fs::File;
    ///
    /// async fn metadata() -> std::io::Result<()> {
    ///     let mut f = File::open("foo.txt").await?;
    ///     let metadata = f.metadata().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn metadata(&self) -> io::Result<Metadata> {
        let file = self.file.clone();
        async_op(move || file.metadata()).await
    }

    /// Creates a new File instance that shares the same underlying file handle
    /// as the existing File instance. Reads, writes, and seeks will affect both
    /// File instances simultaneously.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use ylong_runtime::fs::File;
    ///
    /// async fn try_clone() -> std::io::Result<()> {
    ///     let mut f = File::open("foo.txt").await?;
    ///     let file_copy = f.try_clone().await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn try_clone(&self) -> io::Result<File> {
        let file = self.file.clone();
        let file = async_op(move || file.try_clone()).await?;
        Ok(Self::new(file))
    }

    /// Sets the buffer size limit for [`AsyncRead`] and [`AsyncWrite`]
    /// operation of this file.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn set_file_buffer_limit() {
    /// use ylong_runtime::fs::File;
    /// use ylong_runtime::io::{AsyncReadExt, AsyncWriteExt};
    /// let mut file = File::open("foo.txt").await.unwrap();
    /// file.set_buffer_size_limit(1024 * 1024 * 2);
    /// let mut buf = vec![0; 1024 * 1024 * 16];
    /// // write the buffer in chunks up to 2MB each
    /// let _ = file.write_all(&mut buf).await;
    /// let _ = file.sync_all().await;
    ///
    /// let mut file = File::open("foo.txt").await.unwrap();
    /// file.set_buffer_size_limit(1024 * 1024 * 2);
    /// let mut read_buf = vec![];
    /// // read the file in chunks up to 2MB each
    /// let _ = file.read_to_end(&mut read_buf);
    /// # }
    /// ```
    pub fn set_buffer_size_limit(&mut self, buf_size_limit: usize) {
        self.buf_size_limit = buf_size_limit;
    }

    /// Takes the ownership of the `File` and turns it into a [`std::fs::File`].
    /// Before the transition, any in-flight operations will be completed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::fs::File;
    ///
    /// # async fn file_into_std() -> std::io::Result<()> {
    /// let file = File::open("foo.txt").await?;
    /// let std = file.into_std().await;
    /// # Ok(())
    /// # }
    pub async fn into_std(mut self) -> SyncFile {
        let file = self.inner.get_mut();
        file.flush().await;
        Arc::try_unwrap(self.file).expect("into_std Arc::try_unwrap failed")
    }

    /// Turns the `File` into [`std::fs::File`] immediately without awaiting any
    /// in-flight operation to complete.
    ///
    /// # Errors
    /// This method wil return an error containing the file if some operation
    /// is still in-flight
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ylong_runtime::fs::File;
    ///
    /// # async fn file_try_into_std() -> std::io::Result<()> {
    /// let file = File::open("foo.txt").await?;
    /// let std = file.try_into_std().unwrap();
    /// # Ok(())
    /// # }
    pub fn try_into_std(mut self) -> Result<SyncFile, Self> {
        match Arc::try_unwrap(self.file) {
            Ok(file) => Ok(file),
            Err(arc_file) => {
                self.file = arc_file;
                Err(self)
            }
        }
    }
}

impl From<SyncFile> for File {
    fn from(file: SyncFile) -> Self {
        Self::new(file)
    }
}

cfg_unix! {
    use std::os::unix::io::{AsFd, AsRawFd, FromRawFd, BorrowedFd, RawFd};

    impl AsRawFd for File {
        fn as_raw_fd(&self) -> RawFd {
            self.file.as_raw_fd()
        }
    }

    impl AsFd for File {
        fn as_fd(&self) -> BorrowedFd<'_> {
            unsafe { BorrowedFd::borrow_raw(self.as_raw_fd()) }
        }
    }

    impl FromRawFd for File {
        unsafe fn from_raw_fd(fd: RawFd) -> Self {
            SyncFile::from_raw_fd(fd).into()
        }
    }
}

cfg_windows! {
    use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle, AsHandle, BorrowedHandle};

    impl AsRawHandle for File {
        fn as_raw_handle(&self) -> RawHandle {
            self.file.as_raw_handle()
        }
    }

    impl AsHandle for File {
        fn as_handle(&self) -> BorrowedHandle<'_> {
            unsafe { BorrowedHandle::borrow_raw(self.as_raw_handle()) }
        }
    }

    impl FromRawHandle for File {
        unsafe fn from_raw_handle(handle: RawHandle) -> Self {
            SyncFile::from_raw_handle(handle).into()
        }
    }
}

impl AsyncSeek for File {
    fn poll_seek(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        mut pos: SeekFrom,
    ) -> Poll<io::Result<u64>> {
        let file = self.get_mut();
        let inner = file.inner.get_mut();
        loop {
            match inner.state {
                FileState::Idle(ref mut buf) => {
                    // after each take, buf will be set right back
                    let mut r_buf = buf.take().unwrap();

                    // move the cursor back since there's unread data in the buf
                    let unread = r_buf.drop_unread();
                    if unread != 0 {
                        if let SeekFrom::Current(ref mut idx) = pos {
                            *idx -= unread
                        }
                    }

                    let file = file.file.clone();
                    inner.state =
                        FileState::Seeking(spawn_blocking(&TaskBuilder::new(), move || {
                            let ret = (&*file).seek(pos);
                            (r_buf, ret)
                        }));
                }
                ref mut state => {
                    let op = state.get_op();
                    let res = poll_ready!(Pin::new(state).poll(cx));
                    match op {
                        FileOp::Reading => {}
                        FileOp::Writing => {
                            if let Err(e) = res {
                                // Save the error for the next write.
                                inner.write_err = Some(e.kind());
                            }
                        }
                        FileOp::Seeking => {
                            if let Ok(idx) = res {
                                inner.idx = idx;
                            }
                            return Poll::Ready(res);
                        }
                    }
                }
            }
        }
    }
}

impl AsyncRead for File {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let file = self.get_mut();
        let inner = file.inner.get_mut();

        loop {
            match inner.state {
                FileState::Idle(ref mut file_buf) => {
                    // after each take, buf will be set right back
                    let mut r_buf = file_buf.take().unwrap();
                    // There is still remaining data from the last read, append it to read buf
                    // directly
                    if r_buf.remaining() != 0 {
                        r_buf.append_to(buf);
                        *file_buf = Some(r_buf);
                        return Poll::Ready(Ok(()));
                    }

                    // Make sure there is enough space to read. File_buf's size might be bigger than
                    // the read_buf's size since other thread might also read into the read_buf.
                    r_buf.reserve(buf.remaining(), file.buf_size_limit);

                    // State transition
                    let file = file.file.clone();
                    inner.state =
                        FileState::Reading(spawn_blocking(&TaskBuilder::new(), move || {
                            let ret = r_buf.read(&mut &*file).map(|_| ());
                            (r_buf, ret)
                        }));
                }
                FileState::Reading(ref mut x) => {
                    let (mut file_buf, res) = poll_ready!(Pin::new(x).poll(cx))?;
                    // Append the data inside the file to the read buffer
                    if res.is_ok() {
                        file_buf.append_to(buf);
                    }
                    inner.state = FileState::Idle(Some(file_buf));
                    return Poll::Ready(res);
                }
                FileState::Writing(ref mut x) => {
                    let (file_buf, res) = poll_ready!(Pin::new(x).poll(cx))?;

                    // Save the error for the next write
                    if let Err(e) = res {
                        inner.write_err = Some(e.kind());
                    }
                    inner.state = FileState::Idle(Some(file_buf))
                }
                FileState::Seeking(ref mut x) => {
                    let (file_buf, res) = poll_ready!(Pin::new(x).poll(cx))?;
                    inner.state = FileState::Idle(Some(file_buf));
                    if let Ok(idx) = res {
                        inner.idx = idx;
                    }
                }
            }
        }
    }
}

impl AsyncWrite for File {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let file = self.get_mut();
        let inner = file.inner.get_mut();

        if let Some(e) = inner.write_err {
            return Poll::Ready(Err(e.into()));
        }

        loop {
            match inner.state {
                FileState::Idle(ref mut file_buf) => {
                    // after each take, buf will be set right back
                    let mut w_buf = file_buf.take().unwrap();

                    let unread = w_buf.drop_unread();
                    let n = w_buf.append(buf, file.buf_size_limit);
                    let file = file.file.clone();

                    inner.state =
                        FileState::Writing(spawn_blocking(&TaskBuilder::new(), move || {
                            // Move the cursor back since there's unread data in the buf.
                            if unread != 0 {
                                if let Err(e) = (&*file).seek(SeekFrom::Current(-unread)) {
                                    return (w_buf, Err(e));
                                }
                            }
                            let res = w_buf.write(&mut &*file);
                            (w_buf, res)
                        }));
                    return Poll::Ready(Ok(n));
                }
                ref mut state => {
                    let op = state.get_op();
                    if let Poll::Ready(Err(e)) = Pin::new(state).poll(cx) {
                        if let FileOp::Writing = op {
                            return Poll::Ready(Err(e));
                        }
                    }
                }
            }
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.inner.get_mut().poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_flush(cx)
    }
}

impl FileInner {
    async fn flush(&mut self) {
        if let Err(e) = poll_fn(|cx| self.poll_flush(cx)).await {
            self.write_err = Some(e.kind());
        }
    }

    fn poll_flush(&mut self, cx: &mut Context) -> Poll<io::Result<()>> {
        if let Some(e) = self.write_err {
            return Poll::Ready(Err(e.into()));
        }

        match self.state {
            FileState::Idle(_) => Poll::Ready(Ok(())),
            ref mut state => {
                let op = state.get_op();
                let res = poll_ready!(Pin::new(state).poll(cx));

                match op {
                    FileOp::Writing => Poll::Ready(res.map(|_| ())),
                    _ => Poll::Ready(Ok(())),
                }
            }
        }
    }
}

#[cfg(test)]
mod test {
    use std::io;
    use std::io::SeekFrom;

    use crate::fs::async_file::DEFAULT_BUF_LIMIT;
    use crate::fs::{remove_file, File, OpenOptions};
    use crate::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};

    /// UT test for `set_len`
    ///
    /// # Brief
    ///
    /// 1. Creates a new file.
    /// 2. Removes the file with `set_len()`, check result is Ok(()).
    /// 3. Deletes the file.
    #[test]
    fn ut_fs_file_set_len() {
        crate::block_on(async {
            let file_path = "file11.txt";

            let file = File::create(file_path).await.unwrap();
            let res = file.set_len(10).await;
            assert!(res.is_ok());

            let res = remove_file(file_path).await;
            assert!(res.is_ok());
        });
    }

    /// UT test for `try_clone`
    ///
    /// # Brief
    ///
    /// 1. Creates a new file.
    /// 2. Creates a new File instance with `try_clone()`, check result is
    ///    Ok(()).
    /// 3. Deletes the file.
    #[test]
    fn ut_fs_file_try_clone() {
        crate::block_on(async {
            let file_path = "file12.txt";

            let file = File::create(file_path).await.unwrap();
            let res = file.try_clone().await;
            assert!(res.is_ok());

            let res = remove_file(file_path).await;
            assert!(res.is_ok());
        });
    }

    /// UT test cases for asynchronous file Seek
    ///
    /// # Brief
    /// 1. Generate an asynchronous file IO with create.
    /// 2. Start a task to perform a write operation.
    /// 3. Start another task for seek and read operations.
    #[test]
    fn ut_fs_file_seek() {
        let file_path = "file13.txt";
        let handle = crate::spawn(async move {
            let mut file = File::create(file_path).await.unwrap();
            let buf = vec![65, 66, 67, 68, 69, 70, 71, 72, 73];
            let res = file.write(&buf).await.unwrap();
            assert_eq!(res, 9);
            file.sync_all().await.unwrap();
        });
        crate::block_on(handle).unwrap();

        let handle2 = crate::spawn(async move {
            let mut file = File::open(file_path).await.unwrap();
            let ret = file.seek(SeekFrom::Current(3)).await.unwrap();
            assert_eq!(ret, 3);

            let mut buf = [0; 1];
            let ret = file.read(&mut buf).await.unwrap();
            assert_eq!(ret, 1);
            assert_eq!(buf, [68]);

            let ret = file.seek(SeekFrom::Current(1)).await.unwrap();
            assert_eq!(ret, 5);

            let mut buf = [0; 1];
            let ret = file.read(&mut buf).await.unwrap();
            assert_eq!(ret, 1);
            assert_eq!(buf, [70]);

            let ret = file.seek(SeekFrom::Current(2)).await.unwrap();
            assert_eq!(ret, 8);

            let mut buf = [0; 2];
            let ret = file.read(&mut buf).await.unwrap();
            assert_eq!(ret, 1);
            assert_eq!(buf, [73, 0]);

            let ret = file.seek(SeekFrom::Start(0)).await.unwrap();
            assert_eq!(ret, 0);
            let mut buf = [0; 9];
            let ret = file.read(&mut buf).await.unwrap();
            assert_eq!(ret, 9);
            assert_eq!(buf, [65, 66, 67, 68, 69, 70, 71, 72, 73]);

            let ret = file.seek(SeekFrom::End(-1)).await.unwrap();
            assert_eq!(ret, 8);
            let mut buf = [0; 2];
            let ret = file.read(&mut buf).await.unwrap();
            assert_eq!(ret, 1);
            assert_eq!(buf, [73, 0]);
        });

        crate::block_on(handle2).unwrap();
        std::fs::remove_file(file_path).unwrap();
    }

    /// UT test cases for Asynchronous file set permission
    ///
    /// # Brief
    /// 1. Generate an asynchronous file IO with create.
    /// 2. Asynchronously get the permissions of the file.
    /// 3. Change the permission to read only, set it to this file.
    #[test]
    fn ut_fs_file_set_permission() {
        let file_path = "file14.txt";
        let handle = crate::spawn(async move {
            let file = File::create(file_path).await.unwrap();
            let mut perms = file.metadata().await.unwrap().permissions();
            perms.set_readonly(true);
            let ret = file.set_permissions(perms).await;
            assert!(ret.is_ok());
            let mut perms = file.metadata().await.unwrap().permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let ret = file.set_permissions(perms).await;
            assert!(ret.is_ok());
        });
        crate::block_on(handle).unwrap();
        std::fs::remove_file(file_path).unwrap();
    }

    /// UT test cases for asynchronous file sync
    ///
    /// # Brief
    /// 1. Generate an asynchronous file IO with create.
    /// 2. Call sync_all and sync_data after asynchronous write.
    #[test]
    fn ut_fs_file_sync_all() {
        let file_path = "file15.txt";
        let handle = crate::spawn(async move {
            let mut file = File::create(file_path).await.unwrap();
            let buf = [2; 20000];
            let ret = file.write_all(&buf).await;
            assert!(ret.is_ok());
            let ret = file.sync_all().await;
            assert!(ret.is_ok());

            let buf = [2; 20000];
            let ret = file.write_all(&buf).await;
            assert!(ret.is_ok());
            let ret = file.sync_data().await;
            assert!(ret.is_ok());
        });
        crate::block_on(handle).unwrap();
        std::fs::remove_file(file_path).unwrap();
    }

    /// UT for setting the buffer's size limit
    ///
    /// # Brief
    /// 1. Creates a new file, checks if its buffer size limit equals to
    /// DEFAULT_BUF_LIMIT 2. Sets a new limit for file buffer
    /// 3. Writes the entire buffer into the file
    #[test]
    fn ut_fs_set_buf_size_limit() {
        let file_path = "file16.txt";
        let handle = crate::spawn(async move {
            let mut file = File::create(file_path).await.unwrap();
            assert_eq!(file.buf_size_limit, DEFAULT_BUF_LIMIT);

            file.set_buffer_size_limit(1024 * 1024 * 4);
            assert_eq!(file.buf_size_limit, 1024 * 1024 * 4);

            let buf = [2; 20000];
            let ret = file.write_all(&buf).await;
            assert!(ret.is_ok());
            let ret = file.sync_all().await;
            assert!(ret.is_ok());
        });
        crate::block_on(handle).unwrap();
        std::fs::remove_file(file_path).unwrap();
    }

    /// UT for opening an non-existed file
    ///
    /// # Brief
    /// 1. Open a file that does not exist
    /// 2. Check if the returned error is NotFound
    #[test]
    fn ut_fs_open_fail() {
        let handle = crate::spawn(async move {
            let file = File::open("file_not_exist").await;
            assert_eq!(file.unwrap_err().kind(), io::ErrorKind::NotFound);
        });
        crate::block_on(handle).unwrap();
    }

    /// UT for `into_std`
    ///
    /// # Brief
    /// 1. Creates an async File
    /// 2. Turns the async File into a std File
    /// 3. Check if the std file functions correctly
    #[test]
    fn ut_file_into_std() {
        use std::io::{Read, Seek};

        let file_path = "file17.txt";
        let handle = crate::spawn(async move {
            let mut file = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .open(file_path)
                .await
                .unwrap();

            let res = file.write_all(b"hello").await;
            assert!(res.is_ok());

            let mut std = file.into_std().await;
            let ret = std.seek(SeekFrom::Start(0)).unwrap();
            assert_eq!(ret, 0);
            let mut buf = [0; 5];
            let ret = std.read_exact(&mut buf);
            assert!(ret.is_ok());
            assert_eq!(&buf, b"hello");
        });
        crate::block_on(handle).unwrap();
        std::fs::remove_file(file_path).unwrap();
    }

    /// UT for `try_into_std`
    ///
    /// # Brief
    /// 1. Creates an async File
    /// 2. Uses the async file to write without flushing
    /// 3. Tries to turn the async file into a std file
    /// 4. Check if the attempt fails
    /// 5. Flushes the file
    /// 6. Tries to turn the async file into a std file
    /// 7. Check if the attempt succeeds
    #[test]
    fn ut_file_try_into_std() {
        let file_path = "file18.txt";
        let handle = crate::spawn(async move {
            let mut file = File::create(file_path).await.unwrap();
            let res = file.write_all(b"hello").await;
            assert!(res.is_ok());

            let std = file.try_into_std();
            assert!(std.is_err());

            let mut file = std.unwrap_err();
            let ret = file.flush().await;
            assert!(ret.is_ok());

            let std = file.try_into_std();
            assert!(std.is_ok());
        });
        crate::block_on(handle).unwrap();
        std::fs::remove_file(file_path).unwrap();
    }

    /// UT for `as_raw_fd`
    ///
    /// # Brief
    /// 1. Creates an async File and turns it into a std file
    /// 2. Gets the raw fd from the std file
    /// 3. Turns the std file back to an async file
    /// 4. Check if the fds are equal
    /// 5. Creates a new async file from the raw fd
    /// 6. Check if the fd of the new async file is correct
    #[test]
    #[cfg(unix)]
    fn ut_file_as_raw_fd() {
        use std::os::fd::{AsFd, AsRawFd};
        use std::os::unix::io::FromRawFd;

        let file_path = "file19.txt";
        let handle = crate::spawn(async move {
            let file = File::create(file_path).await.unwrap();
            let std = file.into_std().await;
            let fd = std.as_raw_fd();
            let file = File::from(std);
            let fd2 = file.as_raw_fd();
            assert_eq!(fd, fd2);
            let fd3 = file.as_fd().as_raw_fd();
            assert_eq!(fd, fd3);

            let file2 = unsafe { File::from_raw_fd(fd) };
            let fd4 = file2.as_raw_fd();
            assert_eq!(fd, fd4);
        });

        crate::block_on(handle).unwrap();
        std::fs::remove_file(file_path).unwrap();
    }
}

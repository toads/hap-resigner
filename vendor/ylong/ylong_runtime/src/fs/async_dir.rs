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

use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs::{FileType, Metadata, Permissions};
use std::future::Future;
use std::io;
use std::iter::Fuse;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll::Ready;
use std::task::{Context, Poll};

use crate::fs::{async_op, poll_ready};
use crate::futures::poll_fn;
use crate::spawn::spawn_blocking;
use crate::task::{JoinHandle, TaskBuilder};

const BLOCK_SIZE: usize = 32;

/// Creates a new directory at the given path.
///
/// The async version of [`std::fs::create_dir`]
///
/// # Errors
///
/// In the following situations, the function will return an error, but is not
/// limited to just these cases:
///
/// * The path has already been used.
/// * No permission to create directory at the given path.
/// * A parent directory in the path does not exist. In this case, use
///   [`create_dir_all`] to create the missing parent directory and the target
///   directory at the same time.
///
/// # Examples
///
/// ```no_run
/// use std::io;
///
/// use ylong_runtime::fs;
/// async fn fs_func() -> io::Result<()> {
///     fs::create_dir("/parent/dir").await?;
///     Ok(())
/// }
/// ```
pub async fn create_dir<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let path = path.as_ref().to_owned();
    async_op(move || std::fs::create_dir(path)).await
}

/// Creates a new directory and all of its missing parent directories.
///
/// The async version of [`std::fs::create_dir_all`]
///
/// # Errors
///
/// In the following situations, the function will return an error, but is not
/// limited to just these cases:
///
/// * The path has already been used.
/// * No permission to create directory at the given path.
/// * The missing parent directories can't not be created.
///
/// # Examples
///
/// ```no_run
/// use std::io;
///
/// use ylong_runtime::fs;
/// async fn fs_func() -> io::Result<()> {
///     fs::create_dir_all("/parent/dir").await?;
///     Ok(())
/// }
/// ```
pub async fn create_dir_all<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let path = path.as_ref().to_owned();
    async_op(move || std::fs::create_dir_all(path)).await
}

/// Removes an empty directory.
///
/// The async version of [`std::fs::remove_dir`]
///
/// # Errors
///
/// In the following situations, the function will return an error, but is not
/// limited to just these cases:
///
/// * The directory does not exist.
/// * The given path is not a directory.
/// * No permission to remove directory at the given path.
/// * The directory isn't empty.
///
/// # Examples
///
/// ```no_run
/// use std::io;
///
/// use ylong_runtime::fs;
/// async fn fs_func() -> io::Result<()> {
///     fs::remove_dir("/parent/dir").await?;
///     Ok(())
/// }
/// ```
pub async fn remove_dir<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let path = path.as_ref().to_owned();
    async_op(move || std::fs::remove_dir(path)).await
}

/// Removes a directory and all its contents at the given path.
///
/// The async version of [`std::fs::remove_dir_all`]
///
/// # Errors
///
/// * The directory does not exist.
/// * The given path is not a directory.
/// * No permission to remove directory or its contents at the given path.
///
/// # Examples
///
/// ```no_run
/// use std::io;
///
/// use ylong_runtime::fs;
/// async fn fs_func() -> io::Result<()> {
///     fs::remove_dir_all("/parent/dir").await?;
///     Ok(())
/// }
/// ```
pub async fn remove_dir_all<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let path = path.as_ref().to_owned();
    async_op(move || std::fs::remove_dir_all(path)).await
}

/// Returns an iterator over the entries within a directory.
///
/// The async version of [`std::fs::read_dir`]
///
/// # Errors
///
/// * The directory does not exist.
/// * The given path is not a directory.
/// * No permission to view the contents at the given path.
///
/// # Examples
///
/// ```no_run
/// use std::io;
///
/// use ylong_runtime::fs;
/// async fn fs_func() -> io::Result<()> {
///     let mut dir = fs::read_dir("/parent/dir").await?;
///     assert!(dir.next().await.is_ok());
///     Ok(())
/// }
/// ```
pub async fn read_dir<P: AsRef<Path>>(path: P) -> io::Result<ReadDir> {
    let path = path.as_ref().to_owned();
    async_op(|| {
        let mut std_dir = std::fs::read_dir(path)?.fuse();
        let mut block = VecDeque::with_capacity(BLOCK_SIZE);
        ReadDir::fill_block(&mut std_dir, &mut block);
        Ok(ReadDir::new(std_dir, block))
    })
    .await
}

/// Removes a file from the filesystem.
///
/// The async version of [`std::fs::remove_file`]
///
/// # Errors
///
/// This function will return an error in the following situations, but is not
/// limited to just these cases:
///
/// * Path points to a directory.
/// * The file doesn't exist.
/// * The user lacks permissions to remove the file.
///
/// # Examples
///
/// ```no_run
/// use std::io;
///
/// use ylong_runtime::fs;
/// async fn fs_func() -> io::Result<()> {
///     fs::remove_file("file.txt").await?;
///     Ok(())
/// }
/// ```
pub async fn remove_file<P: AsRef<Path>>(path: P) -> io::Result<()> {
    let path = path.as_ref().to_owned();
    async_op(move || std::fs::remove_file(path)).await
}

/// Rename a file or directory to a new name, replacing the original file if to
/// already exists. This will not work if the new name is on a different mount
/// point.
///
/// The async version of [`std::fs::rename`]
///
/// # Errors
///
/// This function will return an error in the following situations, but is not
/// limited to just these cases:
///
/// * from does not exist.
/// * The user lacks permissions to view contents.
/// * from and to are on separate filesystems.
///
/// # Examples
///
/// ```no_run
/// use std::io;
///
/// use ylong_runtime::fs;
/// async fn fs_func() -> io::Result<()> {
///     fs::rename("file.txt", "new_file.txt").await?;
///     Ok(())
/// }
/// ```
pub async fn rename<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> io::Result<()> {
    let from = from.as_ref().to_owned();
    let to = to.as_ref().to_owned();
    async_op(move || std::fs::rename(from, to)).await
}

/// Copies the contents of one file to another. This function will also copy the
/// permission bits of the original file to the destination file. This function
/// will overwrite the contents of to. Note that if from and to both point to
/// the same file, then the file will likely get truncated by this operation. On
/// success, the total number of bytes copied is returned and it is equal to the
/// length of the to file as reported by metadata.
///
/// The async version of [`std::fs::copy`]
///
/// # Errors
///
/// This function will return an error in the following situations, but is not
/// limited to just these cases:
///
/// * from is neither a regular file nor a symlink to a regular file.
/// * from does not exist.
/// * The current process does not have the permission rights to read from or
///   write to.
///
/// # Examples
///
/// ```no_run
/// use std::io;
///
/// use ylong_runtime::fs;
/// async fn fs_func() -> io::Result<()> {
///     fs::copy("file.txt", "new_file.txt").await?;
///     Ok(())
/// }
/// ```
pub async fn copy<P: AsRef<Path>, Q: AsRef<Path>>(from: P, to: Q) -> io::Result<u64> {
    let from = from.as_ref().to_owned();
    let to = to.as_ref().to_owned();
    async_op(move || std::fs::copy(from, to)).await
}

/// Reads the entire contents of a file into a string.
///
/// The async version of [`std::fs::read_to_string`]
///
/// # Errors
///
/// This function will return an error if path does not already exist.
///
/// # Examples
///
/// ```no_run
/// use std::io;
///
/// use ylong_runtime::fs;
/// async fn read_to_string() -> io::Result<()> {
///     let foo = fs::read_to_string("file.txt").await?;
///     Ok(())
/// }
/// ```
pub async fn read_to_string<P: AsRef<Path>>(path: P) -> io::Result<String> {
    let path = path.as_ref().to_owned();
    async_op(move || std::fs::read_to_string(path)).await
}

/// Reads the entire contents of a file into a bytes vector.
///
/// The async version of [`std::fs::read`]
///
/// # Errors
///
/// This function will return an error if path does not already exist.
///
/// # Examples
///
/// ```no_run
/// use std::io;
///
/// use ylong_runtime::fs;
/// async fn read() -> io::Result<()> {
///     let foo = fs::read("file.txt").await?;
///     Ok(())
/// }
/// ```
pub async fn read<P: AsRef<Path>>(path: P) -> io::Result<Vec<u8>> {
    let path = path.as_ref().to_owned();
    async_op(move || std::fs::read(path)).await
}

/// Writes a slice as the entire contents of a file.
/// This function will create a file if it does not exist, and will entirely
/// replace its contents if it does.
///
/// The async version of [`std::fs::write`]
///
/// # Examples
///
/// ```no_run
/// use std::io;
///
/// use ylong_runtime::fs;
/// async fn write() -> io::Result<()> {
///     fs::write("file.txt", b"Hello world").await?;
///     Ok(())
/// }
/// ```
pub async fn write<P: AsRef<Path>, C: AsRef<[u8]>>(path: P, contents: C) -> io::Result<()> {
    let path = path.as_ref().to_owned();
    let contents = contents.as_ref().to_owned();

    async_op(move || std::fs::write(path, contents)).await
}

/// Reads a symbolic link, returning the file that the link points to.
///
/// The async version of [`std::fs::read_link`]
///
/// # Errors
///
/// This function will return an error in the following situations, but is not
/// limited to just these cases:
///
/// * path is not a symbolic link.
/// * path does not exist.
///
/// # Examples
///
/// ```no_run
/// use std::io;
///
/// use ylong_runtime::fs;
/// async fn read_link() -> io::Result<()> {
///     fs::read_link("file.txt").await?;
///     Ok(())
/// }
/// ```
pub async fn read_link<P: AsRef<Path>>(path: P) -> io::Result<PathBuf> {
    let path = path.as_ref().to_owned();
    async_op(move || std::fs::read_link(path)).await
}

/// Creates a new hard link on the filesystem.
///
/// The async version of [`std::fs::hard_link`]
///
/// # Errors
///
/// This function will return an error in the following situations, but is not
/// limited to just these cases:
///
/// * The original path is not a file or doesn't exist.
///
/// # Examples
///
/// ```no_run
/// use std::io;
///
/// use ylong_runtime::fs;
/// async fn hard_link() -> io::Result<()> {
///     fs::hard_link("file1.txt", "file2.txt").await?;
///     Ok(())
/// }
/// ```
pub async fn hard_link<P: AsRef<Path>, Q: AsRef<Path>>(original: P, link: Q) -> io::Result<()> {
    let original = original.as_ref().to_owned();
    let link = link.as_ref().to_owned();
    async_op(move || std::fs::hard_link(original, link)).await
}

/// Given a path, query the file system to get information about a file,
/// directory, etc. This function will traverse symbolic links to query
/// information about the destination file.
///
/// The async version of [`std::fs::metadata`]
///
/// # Errors
///
/// This function will return an error in the following situations, but is not
/// limited to just these cases:
///
/// * The original path is not a file or doesn't exist.
///
/// # Examples
///
/// ```no_run
/// use std::io;
///
/// use ylong_runtime::fs;
/// async fn metadata() -> io::Result<()> {
///     let data = fs::metadata("/path/file.txt").await?;
///     Ok(())
/// }
/// ```
pub async fn metadata<P: AsRef<Path>>(path: P) -> io::Result<Metadata> {
    let path = path.as_ref().to_owned();
    async_op(move || std::fs::metadata(path)).await
}

/// Queries the metadata about a file without following symlinks.
///
/// The async version of [`std::fs::symlink_metadata`]
///
/// # Errors
///
/// This function will return an error in the following situations, but is not
/// limited to just these cases:
///
/// * The user lacks permissions to perform metadata call on path.
/// * path does not exist.
///
/// # Examples
///
/// ```no_run
/// use std::io;
///
/// use ylong_runtime::fs;
/// async fn symlink_metadata() -> io::Result<()> {
///     fs::symlink_metadata("/path/file.txt").await?;
///     Ok(())
/// }
/// ```
pub async fn symlink_metadata<P: AsRef<Path>>(path: P) -> io::Result<Metadata> {
    let path = path.as_ref().to_owned();
    async_op(move || std::fs::symlink_metadata(path)).await
}

/// Returns the canonical, absolute form of a path with all intermediate
/// components normalized and symbolic links resolved.
///
/// The async version of [`std::fs::canonicalize`]
///
/// # Errors
///
/// This function will return an error in the following situations, but is not
/// limited to just these cases:
///
/// * path does not exist.
/// * A non-final component in path is not a directory.
///
/// # Examples
///
/// ```no_run
/// use std::io;
///
/// use ylong_runtime::fs;
/// async fn canonicalize() -> io::Result<()> {
///     fs::canonicalize("../path/../file.txt").await?;
///     Ok(())
/// }
/// ```
pub async fn canonicalize<P: AsRef<Path>>(path: P) -> io::Result<PathBuf> {
    let path = path.as_ref().to_owned();
    async_op(move || std::fs::canonicalize(path)).await
}

/// Changes the permissions found on a file or a directory.
///
/// The async version of [`std::fs::set_permissions`]
///
/// # Errors
///
/// This function will return an error in the following situations, but is not
/// limited to just these cases:
///
/// * path does not exist.
/// * The user lacks the permission to change attributes of the file.
///
/// # Examples
///
/// ```no_run
/// use std::{fs, io};
/// async fn set_permissions() -> io::Result<()> {
///     let mut perms = ylong_runtime::fs::metadata("file.txt").await?.permissions();
///     perms.set_readonly(true);
///     ylong_runtime::fs::set_permissions("file.txt", perms).await?;
///     Ok(())
/// }
/// ```
pub async fn set_permissions<P: AsRef<Path>>(path: P, perm: Permissions) -> io::Result<()> {
    let path = path.as_ref().to_owned();
    async_op(move || std::fs::set_permissions(path, perm)).await
}

type Entries = (Fuse<std::fs::ReadDir>, VecDeque<io::Result<DirEntry>>);

enum State {
    Available(Box<Option<Entries>>),
    Empty(JoinHandle<Entries>),
}
/// Directory for reading file entries.
///
/// Returned from the [`read_dir`] function of this module and
/// will yield instances of [`io::Result`]<[`DirEntry`]>. A [`DirEntry`]
/// contains information like the entry's path and possibly other metadata.
///
/// # Errors
///
/// Returns [`Err`] if an IO error occurs during iteration.
pub struct ReadDir(State);

impl ReadDir {
    fn new(std_dir: Fuse<std::fs::ReadDir>, block: VecDeque<io::Result<DirEntry>>) -> ReadDir {
        ReadDir(State::Available(Box::new(Some((std_dir, block)))))
    }

    fn fill_block(
        std_dir: &mut Fuse<std::fs::ReadDir>,
        block: &mut VecDeque<io::Result<DirEntry>>,
    ) {
        for res in std_dir.by_ref().take(BLOCK_SIZE) {
            match res {
                Ok(entry) => block.push_back(Ok(DirEntry(Arc::new(entry)))),
                Err(e) => block.push_back(Err(e)),
            }
        }
    }

    fn poll_next(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<Option<DirEntry>>> {
        loop {
            match self.0 {
                State::Available(ref mut dir) => {
                    // before each take, the dir is set
                    let (mut std_dir, mut block) = dir.take().unwrap();
                    match block.pop_front() {
                        Some(Ok(entry)) => {
                            self.0 = State::Available(Box::new(Some((std_dir, block))));
                            return Ready(Ok(Some(entry)));
                        }
                        Some(Err(e)) => {
                            self.0 = State::Available(Box::new(Some((std_dir, block))));
                            return Ready(Err(e));
                        }
                        None => {}
                    }

                    self.0 = State::Empty(spawn_blocking(&TaskBuilder::new(), move || {
                        ReadDir::fill_block(&mut std_dir, &mut block);
                        (std_dir, block)
                    }));
                }
                State::Empty(ref mut handle) => {
                    let (std_dir, mut block) = poll_ready!(Pin::new(handle).poll(cx))?;
                    let res = match block.pop_front() {
                        Some(Ok(entry)) => Ok(Some(entry)),
                        Some(Err(e)) => Err(e),
                        None => Ok(None),
                    };
                    self.0 = State::Available(Box::new(Some((std_dir, block))));
                    return Ready(res);
                }
            }
        }
    }

    /// Returns the next entry in the directory.
    ///
    /// # Return value
    /// The function returns:
    /// * `Ok(Some(entry))` entry is an entry in the directory.
    /// * `Ok(None)` if there is no more entries in the directory.
    /// * `Err(e)` if an IO error occurred.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::io;
    ///
    /// use ylong_runtime::fs;
    /// async fn fs_func() -> io::Result<()> {
    ///     let mut dir = fs::read_dir("/parent/dir").await?;
    ///     assert!(dir.next().await.is_ok());
    ///     Ok(())
    /// }
    /// ```
    pub async fn next(&mut self) -> io::Result<Option<DirEntry>> {
        poll_fn(|cx| self.poll_next(cx)).await
    }
}

/// Entries returned by the [`ReadDir::next`].
///
/// Represents an entry inside of a directory on the filesystem.
/// Each entry can be inspected via methods to learn about the full path
/// or possibly other metadata through per-platform extension traits.
pub struct DirEntry(Arc<std::fs::DirEntry>);

impl DirEntry {
    /// Returns the full path to the file represented by this entry.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::io;
    ///
    /// use ylong_runtime::fs;
    ///
    /// async fn fs_func() -> io::Result<()> {
    ///     let mut dir = fs::read_dir("/parent/dir").await?;
    ///     while let Some(entry) = dir.next().await? {
    ///         println!("{:?}", entry.path());
    ///     }
    ///     Ok(())
    /// }
    /// ```
    ///
    /// This prints output like:
    ///
    /// ```text
    /// "/parent/dir/some.txt"
    /// "/parent/dir/rust.rs"
    /// ```
    pub fn path(&self) -> PathBuf {
        self.0.path()
    }

    /// Returns the name of the file represented by this entry.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::io;
    ///
    /// use ylong_runtime::fs;
    ///
    /// async fn fs_func() -> io::Result<()> {
    ///     let mut dir = fs::read_dir("/parent/dir").await?;
    ///     while let Some(entry) = dir.next().await? {
    ///         println!("{:?}", entry.file_name());
    ///     }
    ///     Ok(())
    /// }
    /// ```
    pub fn file_name(&self) -> OsString {
        self.0.file_name()
    }

    /// Returns the metadata for the file represented by this entry.
    ///
    /// This function won't traverse symlinks if this entry points
    /// at a symlink.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::io;
    ///
    /// use ylong_runtime::fs;
    ///
    /// async fn fs_func() -> io::Result<()> {
    ///     let mut dir = fs::read_dir("/parent/dir").await?;
    ///     while let Some(entry) = dir.next().await? {
    ///         if let Ok(metadata) = entry.metadata().await {
    ///             println!("{:?}", metadata.permissions());
    ///         }
    ///     }
    ///     Ok(())
    /// }
    /// ```
    pub async fn metadata(&self) -> io::Result<Metadata> {
        let entry = self.0.clone();
        async_op(move || entry.metadata()).await
    }

    /// Returns the file type for the file represented by this entry.
    ///
    /// This function won't traverse symlinks if this entry points
    /// at a symlink.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::io;
    ///
    /// use ylong_runtime::fs;
    ///
    /// async fn fs_func() -> io::Result<()> {
    ///     let mut dir = fs::read_dir("/parent/dir").await?;
    ///     while let Some(entry) = dir.next().await? {
    ///         if let Ok(file_type) = entry.file_type().await {
    ///             println!("{:?}", file_type);
    ///         }
    ///     }
    ///     Ok(())
    /// }
    /// ```
    pub async fn file_type(&self) -> io::Result<FileType> {
        let entry = self.0.clone();
        async_op(move || entry.file_type()).await
    }
}

#[cfg(test)]
mod test {
    use std::io;

    use crate::fs::{
        canonicalize, copy, create_dir, hard_link, metadata, read, read_dir, read_link,
        read_to_string, remove_dir_all, remove_file, rename, set_permissions, symlink_metadata,
        write, File,
    };

    /// UT test for `remove_file`
    ///
    /// # Brief
    ///
    /// 1. Create a new file.
    /// 2. Remove the file with `remove_file()`, check result is Ok(()).
    #[test]
    fn ut_fs_create_remove_file() {
        crate::block_on(async {
            let file_path = "file0.txt";

            File::create(file_path).await.unwrap();
            let res = remove_file(file_path).await;

            assert!(res.is_ok());
        });
    }

    /// UT test for creating
    ///
    /// # Brief
    ///
    /// 1. Create a new directory whose parent doesn't exist.
    /// 2. Check if the returned error is NotFound.
    #[test]
    fn ut_fs_create_dir_fail() {
        crate::block_on(async {
            let ret = create_dir("non-existed_parent/non_existed_child").await;
            assert_eq!(ret.unwrap_err().kind(), io::ErrorKind::NotFound);
        })
    }

    /// UT test for `rename`
    ///
    /// # Brief
    ///
    /// 1. Create a new file.
    /// 2. Rename the file with `rename()`.
    /// 3. Delete the new file.
    #[test]
    fn ut_fs_rename() {
        crate::block_on(async {
            let file_path = "file1.txt";

            File::create(file_path).await.unwrap();
            let res = rename(file_path, "new_file1.txt").await;
            assert!(res.is_ok());

            let res = remove_file("new_file1.txt").await;
            assert!(res.is_ok());
        });
    }

    /// UT test for `write()` and `read_to_string()`
    ///
    /// # Brief
    ///
    /// 1. Create a new file.
    /// 2. Write the file with `write()`.
    /// 3. Read the file with `read_to_string()`, check whether it's correct.
    /// 4. Delete the file.
    #[test]
    fn ut_fs_write_and_read_to_string() {
        crate::block_on(async {
            let input = "Hello world";
            let file_path = "file2.txt";

            File::create(file_path).await.unwrap();

            let res = write(file_path, input.as_bytes()).await;
            assert!(res.is_ok());
            let s = read_to_string(file_path).await.unwrap();
            assert_eq!(s, input);

            let res = remove_file(file_path).await;
            assert!(res.is_ok());
        });
    }

    /// UT test for `read()` and `write()`
    ///
    /// # Brief
    ///
    /// 1. Create a new file.
    /// 2. Write the file with `write()`.
    /// 3. Read the file with `read()`, check whether it's correct.
    /// 4. Delete the file.
    #[test]
    fn ut_fs_write_and_read() {
        crate::block_on(async {
            let input = "Hello world";
            let file_path = "file3.txt";

            File::create(file_path).await.unwrap();

            let res = write(file_path, input.as_bytes()).await;
            assert!(res.is_ok());
            let buf = read(file_path).await.unwrap();
            let s = String::from_utf8(buf).expect("Found invalid UTF-8");
            assert_eq!(s, input);

            let res = remove_file(file_path).await;
            assert!(res.is_ok());
        });
    }

    /// UT test for `read_link()`
    ///
    /// # Brief
    ///
    /// 1. Read a symbolic link with read_link().
    /// 2. Check whether the result is correct.
    #[test]
    fn ut_fs_read_link() {
        crate::block_on(async {
            let file_path = "file4.txt";

            let res = read_link(file_path).await;
            assert!(res.is_err());
        });
    }

    /// UT test for `copy()`
    ///
    /// # Brief
    ///
    /// 1. Create a new file.
    /// 2. Copy the file with `copy()`.
    /// 3. Check whether the result is correct.
    /// 4. Delete two files.
    #[test]
    fn ut_fs_copy() {
        crate::block_on(async {
            let file_path = "file5.txt";

            File::create(file_path).await.unwrap();

            let res = copy(file_path, "new_file5.txt").await;
            assert!(res.is_ok());

            let res = remove_file(file_path).await;
            assert!(res.is_ok());
            let res = remove_file("new_file5.txt").await;
            assert!(res.is_ok());
        });
    }

    /// UT test for `hard_link()`
    ///
    /// # Brief
    ///
    /// 1. Create a new file.
    /// 2. Create a new hard link on the filesystem with `hard_link()`.
    /// 3. Check whether the result is correct.
    /// 4. Delete two files.
    #[test]
    fn ut_fs_hard_link() {
        crate::block_on(async {
            let file_path = "file6.txt";

            File::create(file_path).await.unwrap();

            let res = hard_link(file_path, "new_file6.txt").await;
            assert!(res.is_ok());

            let res = remove_file(file_path).await;
            assert!(res.is_ok());
            let res = remove_file("new_file6.txt").await;
            assert!(res.is_ok());
        });
    }

    /// UT test for `metadata()`
    ///
    /// # Brief
    ///
    /// 1. Create a new file.
    /// 2. Get information about this file with `metadata()`.
    /// 3. Check whether the result is correct.
    /// 4. Delete the file.
    #[test]
    fn ut_fs_metadata() {
        crate::block_on(async {
            let file_path = "file7.txt";

            File::create(file_path).await.unwrap();

            let res = metadata(file_path).await;
            assert!(res.is_ok());

            let res = remove_file(file_path).await;
            assert!(res.is_ok());
        });
    }

    /// UT test for `canonicalize()`
    ///
    /// # Brief
    ///
    /// 1. Create a new file.
    /// 2. Get the canonical, absolute form of a path with all intermediate
    ///    components normalized and symbolic links resolved with
    ///    `canonicalize()`.
    /// 3. Check whether the result is correct.
    /// 4. Delete the file.
    #[test]
    fn ut_fs_canonicalize() {
        crate::block_on(async {
            let file_path = "file8.txt";
            File::create(file_path).await.unwrap();

            let res = canonicalize(file_path).await;
            assert!(res.is_ok());

            let res = remove_file(file_path).await;
            assert!(res.is_ok());
        });
    }

    /// UT test for `symlink_metadata()`
    ///
    /// # Brief
    ///
    /// 1. Create a new file.
    /// 2. Query the metadata about a file without following symlinks with
    ///    `symlink_metadata()`.
    /// 3. Check whether the result is correct.
    /// 4. Delete the file.
    #[test]
    fn ut_fs_symlink_metadata() {
        crate::block_on(async {
            let file_path = "file9.txt";
            File::create(file_path).await.unwrap();

            let res = symlink_metadata(file_path).await;
            assert!(res.is_ok());

            let res = remove_file(file_path).await;
            assert!(res.is_ok());
        });
    }

    /// UT test for `set_permissions()`
    ///
    /// # Brief
    ///
    /// 1. Create a new file.
    /// 2. Set file as readonly with `set_permissions()`.
    /// 3. Check whether the result is correct.
    /// 4. Delete the file.
    #[test]
    fn ut_fs_set_permissions() {
        crate::block_on(async {
            let file_path = "file10.txt";
            File::create(file_path).await.unwrap();

            let mut perms = metadata(file_path).await.unwrap().permissions();
            perms.set_readonly(true);
            let res = set_permissions(file_path, perms).await;
            assert!(res.is_ok());

            let mut perms = metadata(file_path).await.unwrap().permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let res = set_permissions(file_path, perms).await;
            assert!(res.is_ok());

            let res = remove_file(file_path).await;
            assert!(res.is_ok());
        });
    }

    /// UT test cases for directory operations.
    ///
    /// # Brief
    /// 1. Create a new directory.
    /// 2. Create two files to read.
    /// 3. Read the directory and check the name of files.
    /// 4. Delete the directory and files in it.
    #[test]
    fn ut_async_dir_read() {
        let handle = crate::spawn(async move {
            let _ = create_dir("dir_test1").await;
            File::create("dir_test1/test1.txt").await.unwrap();
            File::create("dir_test1/test2.txt").await.unwrap();
            let mut dir = read_dir("dir_test1").await.unwrap();
            let entry = dir.next().await.unwrap().unwrap();
            assert!(!entry.file_type().await.unwrap().is_dir());
            assert!(entry.file_type().await.unwrap().is_file());
            assert!(entry.file_name().into_string().unwrap().contains("test"));
            let entry = dir.next().await.unwrap().unwrap();
            assert!(!entry.metadata().await.unwrap().is_dir());
            assert!(entry.metadata().await.unwrap().is_file());
            assert!(!entry.metadata().await.unwrap().permissions().readonly());
            assert!(entry.file_name().into_string().unwrap().contains("test"));
            assert!(dir.next().await.unwrap().is_none());
            assert!(remove_dir_all("dir_test1").await.is_ok());
        });
        crate::block_on(handle).unwrap();
    }
}

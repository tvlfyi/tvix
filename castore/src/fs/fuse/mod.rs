use std::{io, path::Path, sync::Arc};

use fuse_backend_rs::{api::filesystem::FileSystem, transport::FuseSession};
use parking_lot::Mutex;
use threadpool::ThreadPool;
use tracing::{error, instrument};

#[cfg(test)]
mod tests;

struct FuseServer<FS>
where
    FS: FileSystem + Sync + Send,
{
    server: Arc<fuse_backend_rs::api::server::Server<Arc<FS>>>,
    channel: fuse_backend_rs::transport::FuseChannel,
}

#[cfg(target_os = "macos")]
const BADFD: libc::c_int = libc::EBADF;
#[cfg(target_os = "linux")]
const BADFD: libc::c_int = libc::EBADFD;

impl<FS> FuseServer<FS>
where
    FS: FileSystem + Sync + Send,
{
    fn start(&mut self) -> io::Result<()> {
        while let Some((reader, writer)) = self
            .channel
            .get_request()
            .map_err(|_| io::Error::from_raw_os_error(libc::EINVAL))?
        {
            if let Err(e) = self
                .server
                .handle_message(reader, writer.into(), None, None)
            {
                match e {
                    // This indicates the session has been shut down.
                    fuse_backend_rs::Error::EncodeMessage(e) if e.raw_os_error() == Some(BADFD) => {
                        break;
                    }
                    error => {
                        error!(?error, "failed to handle fuse request");
                        continue;
                    }
                }
            }
        }
        Ok(())
    }
}

/// Starts a [Filesystem] with the specified number of threads, and provides
/// functions to unmount, and wait for it to have completed.
#[derive(Clone)]
pub struct FuseDaemon {
    session: Arc<Mutex<FuseSession>>,
    threads: Arc<ThreadPool>,
}

impl FuseDaemon {
    #[instrument(skip(fs, mountpoint), fields(mountpoint=?mountpoint), err)]
    pub fn new<FS, P>(
        fs: FS,
        mountpoint: P,
        num_threads: usize,
        allow_other: bool,
    ) -> Result<Self, io::Error>
    where
        FS: FileSystem + Sync + Send + 'static,
        P: AsRef<Path> + std::fmt::Debug,
    {
        let server = Arc::new(fuse_backend_rs::api::server::Server::new(Arc::new(fs)));

        let mut session = FuseSession::new(mountpoint.as_ref(), "tvix-store", "", true)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        #[cfg(target_os = "linux")]
        session.set_allow_other(allow_other);
        session
            .mount()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        // construct a thread pool
        let threads = threadpool::Builder::new()
            .num_threads(num_threads)
            .thread_name("fuse_server".to_string())
            .build();

        for _ in 0..num_threads {
            // for each thread requested, create and start a FuseServer accepting requests.
            let mut server = FuseServer {
                server: server.clone(),
                channel: session
                    .new_channel()
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?,
            };

            threads.execute(move || {
                let _ = server.start();
            });
        }

        Ok(FuseDaemon {
            session: Arc::new(Mutex::new(session)),
            threads: Arc::new(threads),
        })
    }

    /// Waits for all threads to finish.
    #[instrument(skip_all)]
    pub fn wait(&self) {
        self.threads.join()
    }

    /// Send the unmount command, and waits for all threads to finish.
    #[instrument(skip_all, err)]
    pub fn unmount(&self) -> Result<(), io::Error> {
        // Send the unmount command.
        self.session
            .lock()
            .umount()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        self.wait();
        Ok(())
    }
}

impl Drop for FuseDaemon {
    fn drop(&mut self) {
        if let Err(error) = self.unmount() {
            error!(?error, "failed to unmont fuse filesystem")
        }
    }
}

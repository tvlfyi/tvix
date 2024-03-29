use std::{io, path::Path, sync::Arc, thread};

use fuse_backend_rs::{api::filesystem::FileSystem, transport::FuseSession};
use tracing::{error, instrument};

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

pub struct FuseDaemon {
    session: FuseSession,
    threads: Vec<thread::JoinHandle<()>>,
}

impl FuseDaemon {
    #[instrument(skip(fs, mountpoint), fields(mountpoint=?mountpoint), err)]
    pub fn new<FS, P>(
        fs: FS,
        mountpoint: P,
        threads: usize,
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
        let mut join_handles = Vec::with_capacity(threads);
        for _ in 0..threads {
            let mut server = FuseServer {
                server: server.clone(),
                channel: session
                    .new_channel()
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?,
            };
            let join_handle = thread::Builder::new()
                .name("fuse_server".to_string())
                .spawn(move || {
                    let _ = server.start();
                })?;
            join_handles.push(join_handle);
        }

        Ok(FuseDaemon {
            session,
            threads: join_handles,
        })
    }

    #[instrument(skip_all, err)]
    pub fn unmount(&mut self) -> Result<(), io::Error> {
        self.session
            .umount()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

        for thread in self.threads.drain(..) {
            thread.join().map_err(|_| {
                io::Error::new(io::ErrorKind::Other, "failed to join fuse server thread")
            })?;
        }

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

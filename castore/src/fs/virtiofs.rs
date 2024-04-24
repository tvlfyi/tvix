use std::{
    convert, error, fmt, io,
    ops::Deref,
    path::Path,
    sync::{Arc, MutexGuard, RwLock},
};

use fuse_backend_rs::{
    api::{filesystem::FileSystem, server::Server},
    transport::{FsCacheReqHandler, Reader, VirtioFsWriter},
};
use tracing::error;
use vhost::vhost_user::{
    Listener, SlaveFsCacheReq, VhostUserProtocolFeatures, VhostUserVirtioFeatures,
};
use vhost_user_backend::{VhostUserBackendMut, VhostUserDaemon, VringMutex, VringState, VringT};
use virtio_bindings::bindings::virtio_ring::{
    VIRTIO_RING_F_EVENT_IDX, VIRTIO_RING_F_INDIRECT_DESC,
};
use virtio_queue::QueueT;
use vm_memory::{GuestAddressSpace, GuestMemoryAtomic, GuestMemoryMmap};
use vmm_sys_util::epoll::EventSet;

const VIRTIO_F_VERSION_1: u32 = 32;
const NUM_QUEUES: usize = 2;
const QUEUE_SIZE: usize = 1024;

#[derive(Debug)]
enum Error {
    /// Failed to handle non-input event.
    HandleEventNotEpollIn,
    /// Failed to handle unknown event.
    HandleEventUnknownEvent,
    /// Invalid descriptor chain.
    InvalidDescriptorChain,
    /// Failed to handle filesystem requests.
    #[allow(dead_code)]
    HandleRequests(fuse_backend_rs::Error),
    /// Failed to construct new vhost user daemon.
    NewDaemon,
    /// Failed to start the vhost user daemon.
    StartDaemon,
    /// Failed to wait for the vhost user daemon.
    WaitDaemon,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "vhost_user_fs_error: {self:?}")
    }
}

impl error::Error for Error {}

impl convert::From<Error> for io::Error {
    fn from(e: Error) -> Self {
        io::Error::new(io::ErrorKind::Other, e)
    }
}

struct VhostUserFsBackend<FS>
where
    FS: FileSystem + Send + Sync,
{
    server: Arc<Server<Arc<FS>>>,
    event_idx: bool,
    guest_mem: GuestMemoryAtomic<GuestMemoryMmap>,
    cache_req: Option<SlaveFsCacheReq>,
}

impl<FS> VhostUserFsBackend<FS>
where
    FS: FileSystem + Send + Sync,
{
    fn process_queue(&mut self, vring: &mut MutexGuard<VringState>) -> std::io::Result<bool> {
        let mut used_descs = false;

        while let Some(desc_chain) = vring
            .get_queue_mut()
            .pop_descriptor_chain(self.guest_mem.memory())
        {
            let memory = desc_chain.memory();
            let reader = Reader::from_descriptor_chain(memory, desc_chain.clone())
                .map_err(|_| Error::InvalidDescriptorChain)?;
            let writer = VirtioFsWriter::new(memory, desc_chain.clone())
                .map_err(|_| Error::InvalidDescriptorChain)?;

            self.server
                .handle_message(
                    reader,
                    writer.into(),
                    self.cache_req
                        .as_mut()
                        .map(|req| req as &mut dyn FsCacheReqHandler),
                    None,
                )
                .map_err(Error::HandleRequests)?;

            // TODO: Is len 0 correct?
            if let Err(error) = vring
                .get_queue_mut()
                .add_used(memory, desc_chain.head_index(), 0)
            {
                error!(?error, "failed to add desc back to ring");
            }

            // TODO: What happens if we error out before here?
            used_descs = true;
        }

        let needs_notification = if self.event_idx {
            match vring
                .get_queue_mut()
                .needs_notification(self.guest_mem.memory().deref())
            {
                Ok(needs_notification) => needs_notification,
                Err(error) => {
                    error!(?error, "failed to check if queue needs notification");
                    true
                }
            }
        } else {
            true
        };

        if needs_notification {
            if let Err(error) = vring.signal_used_queue() {
                error!(?error, "failed to signal used queue");
            }
        }

        Ok(used_descs)
    }
}

impl<FS> VhostUserBackendMut<VringMutex> for VhostUserFsBackend<FS>
where
    FS: FileSystem + Send + Sync,
{
    fn num_queues(&self) -> usize {
        NUM_QUEUES
    }

    fn max_queue_size(&self) -> usize {
        QUEUE_SIZE
    }

    fn features(&self) -> u64 {
        1 << VIRTIO_F_VERSION_1
            | 1 << VIRTIO_RING_F_INDIRECT_DESC
            | 1 << VIRTIO_RING_F_EVENT_IDX
            | VhostUserVirtioFeatures::PROTOCOL_FEATURES.bits()
    }

    fn protocol_features(&self) -> VhostUserProtocolFeatures {
        VhostUserProtocolFeatures::MQ | VhostUserProtocolFeatures::SLAVE_REQ
    }

    fn set_event_idx(&mut self, enabled: bool) {
        self.event_idx = enabled;
    }

    fn update_memory(&mut self, _mem: GuestMemoryAtomic<GuestMemoryMmap>) -> std::io::Result<()> {
        // This is what most the vhost user implementations do...
        Ok(())
    }

    fn set_slave_req_fd(&mut self, cache_req: SlaveFsCacheReq) {
        self.cache_req = Some(cache_req);
    }

    fn handle_event(
        &mut self,
        device_event: u16,
        evset: vmm_sys_util::epoll::EventSet,
        vrings: &[VringMutex],
        _thread_id: usize,
    ) -> std::io::Result<bool> {
        if evset != EventSet::IN {
            return Err(Error::HandleEventNotEpollIn.into());
        }

        let mut queue = match device_event {
            // High priority queue
            0 => vrings[0].get_mut(),
            // Regurlar priority queue
            1 => vrings[1].get_mut(),
            _ => {
                return Err(Error::HandleEventUnknownEvent.into());
            }
        };

        if self.event_idx {
            loop {
                queue
                    .get_queue_mut()
                    .enable_notification(self.guest_mem.memory().deref())
                    .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;
                if !self.process_queue(&mut queue)? {
                    break;
                }
            }
        } else {
            self.process_queue(&mut queue)?;
        }

        Ok(false)
    }
}

pub fn start_virtiofs_daemon<FS, P>(fs: FS, socket: P) -> io::Result<()>
where
    FS: FileSystem + Send + Sync + 'static,
    P: AsRef<Path>,
{
    let guest_mem = GuestMemoryAtomic::new(GuestMemoryMmap::new());

    let server = Arc::new(fuse_backend_rs::api::server::Server::new(Arc::new(fs)));

    let backend = Arc::new(RwLock::new(VhostUserFsBackend {
        server,
        guest_mem: guest_mem.clone(),
        event_idx: false,
        cache_req: None,
    }));

    let listener = Listener::new(socket, true).unwrap();

    let mut fs_daemon =
        VhostUserDaemon::new(String::from("vhost-user-fs-tvix-store"), backend, guest_mem)
            .map_err(|_| Error::NewDaemon)?;

    fs_daemon.start(listener).map_err(|_| Error::StartDaemon)?;

    fs_daemon.wait().map_err(|_| Error::WaitDaemon)?;

    Ok(())
}

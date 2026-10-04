//! Ring buffer consumer (P3). Reads wire frames from shared memory.

use anyhow::Result;
use std::os::fd::RawFd;
use tracing::{info, warn};

const OFG_FRAMES_OFFSET: usize = 128;
const OFG_WIRE_MAGIC: u32 = 0x4F464731;

pub struct RingConsumer {
    ptr: *const u8,
    size: usize,
    read_offset: usize,
    data_fd: RawFd,
    space_fd: RawFd,
    frame_count: u64,
    node_count: u64,
    edge_count: u64,
}

impl RingConsumer {
    pub fn new(ptr: *const u8, size: usize, data_fd: RawFd, space_fd: RawFd) -> Self {
        let read_offset = unsafe {
            (ptr.add(64) as *const u64).read_volatile() as usize // initial read_offset = 0
        };
        Self {
            ptr, size, read_offset, data_fd, space_fd,
            frame_count: 0, node_count: 0, edge_count: 0,
        }
    }

    /// Block until new data is available, then process all frames.
    /// Returns (frame_count, node_count, edge_count).
    pub fn process_available(&mut self) -> Result<(u64, u64, u64)> {
        // Read eventfd to wait for data.
        let mut val: u64 = 0;
        let n = unsafe { libc::read(self.data_fd, &mut val as *mut u64 as *mut libc::c_void, 8) };
        if n < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::WouldBlock { return Ok((0, 0, 0)); }
            anyhow::bail!("eventfd read: {}", e);
        }

        // Read write_offset (producer position).
        let write_offset = unsafe {
            (self.ptr.add(64 + 8) as *const u64).read_volatile() as usize
        };

        // Process frames between read_offset and write_offset.
        while self.read_offset != write_offset {
            self.read_one_frame()?;
        }

        // Signal space back.
        let one: u64 = 1;
        unsafe { libc::write(self.space_fd, &one as *const u64 as *const libc::c_void, 8); }

        Ok((self.frame_count, self.node_count, self.edge_count))
    }

    fn read_one_frame(&mut self) -> Result<()> {
        let cap = self.size - OFG_FRAMES_OFFSET;
        let pos = self.read_offset % cap;
        let base = self.ptr.wrapping_add(OFG_FRAMES_OFFSET + pos);

        // Read header.
        let magic = unsafe { (base as *const u32).read_volatile() };
        if magic != OFG_WIRE_MAGIC { anyhow::bail!("bad magic {:#x}", magic); }

        let record_kind = unsafe { base.add(6) as *const u16 };
        let record_kind = unsafe { record_kind.read_volatile() };

        let payload_len = unsafe { (base.add(24) as *const u64).read_volatile() } as usize;
        let total_len   = unsafe { (base.add(32) as *const u64).read_volatile() } as usize;
        let rec_count   = unsafe { (base.add(40) as *const u32).read_volatile() } as usize;

        self.frame_count += 1;
        match record_kind {
            1 => info!("  SNAPSHOT_BEGIN"),
            2 => { self.node_count += rec_count as u64; }
            3 => { self.edge_count += rec_count as u64; }
            7 => info!("  SNAPSHOT_END (nodes={} edges={})", self.node_count, self.edge_count),
            8 => warn!("  FATAL_ERROR"),
            9 => {} // heartbeat
            _ => warn!("  unknown frame kind {}", record_kind),
        }

        self.read_offset = (self.read_offset + total_len) % cap;
        Ok(())
    }
}

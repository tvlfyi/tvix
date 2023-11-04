use magic_buffer::MagicBuffer;
use std::cell::Cell;

/// Buffer is a FIFO queue for bytes, built on a ring buffer.
/// It always provides contiguous slices for both the readable and writable parts,
/// using an underlying buffer that is "mirrored" in virtual memory.
pub struct Buffer {
    buffer: MagicBuffer,
    /// first readable byte
    head: Cell<usize>,
    /// first writable byte
    tail: usize,
}

// SAFETY: MagicBuffer isn't bound to a thread, and neither are any of the other fields.
// MagicBuffer ought to be Send+Sync itself, upstream PR at https://github.com/sklose/magic-buffer/pull/4
unsafe impl Send for Buffer {}

impl Buffer {
    /// Allocate a fresh buffer, with the specified capacity.
    /// The buffer can contain at most `capacity - 1` bytes.
    /// The capacity must be a power of two, and at least [Buffer::min_len].
    pub fn new(capacity: usize) -> Buffer {
        Buffer {
            // MagicBuffer::new verifies that `capacity` is a power of two,
            // and at least MagicBuffer::min_len().
            buffer: MagicBuffer::new(capacity).unwrap(),
            // `head == tail` means the buffer is empty.
            // In order to ensure that this remains unambiguous,
            // the buffer can only be filled with capacity-1 bytes.
            head: Cell::new(0),
            tail: 0,
        }
    }

    /// Returns the minimum buffer capacity.
    /// This depends on the operating system and architecture.
    pub fn min_capacity() -> usize {
        MagicBuffer::min_len()
    }

    /// Return the capacity of the buffer.
    /// This is equal to `self.data().len() + self.space().len() + 1`.
    pub fn capacity(&self) -> usize {
        self.buffer.len()
    }

    /// Return the valid, readable data in the buffer.
    pub fn data(&self) -> &[u8] {
        let len = self.buffer.len();
        let head = self.head.get();

        if head <= self.tail {
            &self.buffer[head..self.tail]
        } else {
            &self.buffer[head..self.tail + len]
        }
    }

    /// Mark `read_len` bytes of the readable data as consumed, freeing the space.
    pub fn consume(&self, read_len: usize) {
        debug_assert!(read_len <= self.data().len());
        let mut head = self.head.get();
        head += read_len;
        head &= self.buffer.len() - 1;
        self.head.set(head);
    }

    /// Return the empty, writable space in the buffer.
    pub fn space(&mut self) -> &mut [u8] {
        let len = self.buffer.len();
        let head = self.head.get();

        if head <= self.tail {
            &mut self.buffer[self.tail..head + len - 1]
        } else {
            &mut self.buffer[self.tail..head - 1]
        }
    }

    /// Mark `written_len` bytes of the writable space as valid, readable data.
    pub fn commit(&mut self, written_len: usize) {
        debug_assert!(written_len <= self.space().len());
        self.tail += written_len;
        self.tail &= self.buffer.len() - 1;
    }
}

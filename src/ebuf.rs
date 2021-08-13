use std::{io, ptr};
use io_uring::{opcode, squeue, types, IoUring, SubmissionQueue, CompletionQueue};
use io_uring::ownedsplit::*;
use slab::Slab;
use std::time::Duration;
use std::num::NonZeroU8;
use libc;
use std::mem;
use std::sync::Mutex;
use std::cell::{Cell, UnsafeCell, RefCell};
use eventfd::EventFD;
use types::Timespec;
use concurrent_queue::ConcurrentQueue;
use std::sync::mpsc::{channel, Sender, Receiver};



thread_local! {
    pub static ENTRY_BUFFER: UnsafeCell<Vec<squeue::Entry>> = UnsafeCell::new(Vec::with_capacity(1024));
}
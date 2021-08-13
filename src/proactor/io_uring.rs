use std::{io, ptr};
use io_uring::{opcode, squeue, types, IoUring, SubmissionQueue, CompletionQueue};
use io_uring::ownedsplit::*;
use slab::Slab;
use std::time::Duration;
use std::num::NonZeroU8;
use libc;
use std::os::unix::io::{AsRawFd, RawFd};
use std::mem;
use std::sync::Mutex;
use std::cell::{Cell, UnsafeCell, RefCell};
use eventfd::EventFD;
use types::Timespec;
use concurrent_queue::ConcurrentQueue;
use std::sync::mpsc::{channel, Sender, Receiver};

pub struct Event {
    pub ret: i32,
    pub user_data: u64,
}

pub struct Reactor {
    submitter: SubmitterUring,
    sq: UnsafeCell<SubmissionUring>,
    cq: UnsafeCell<CompletionUring>,
    backlog: Vec<squeue::Entry>,
    eventfd: EventFD,
    buf: [u8; 8],
    has_timeout_entry: Mutex<RefCell<bool>>,
    timespec: Mutex<RefCell<Timespec>>,
    sender: Sender<squeue::Entry>,
}


unsafe impl Sync for Reactor {}

use once_cell::sync::Lazy;

static FD: Lazy<RawFd> = Lazy::new(|| {
    let ring = IoUring::new(64).unwrap();
    let fd = ring.as_raw_fd();
    mem::forget(ring);
    fd
});

impl Reactor {
    pub fn new() -> io::Result<Reactor> {
        let mut ring = IoUring::builder().setup_attach_wq(FD.clone()).build(256)?;
        let (submitter, mut sq, mut cq) = ring.owned_split();

        let (sender, receiver) = channel::<squeue::Entry>();



        let reactor = Reactor{
            submitter, 
            sq: UnsafeCell::new(sq), 
            cq: UnsafeCell::new(cq), 
            backlog: Vec::with_capacity(256),
            eventfd: EventFD::new(0, 0)?,
            buf: [0u8; 8],
            has_timeout_entry: Mutex::new(RefCell::new(false)),
            timespec: Mutex::new(RefCell::new(Timespec::new())),
            sender,
        };
        reactor.register_eventfd();

        Ok(reactor)
    }

    fn register_eventfd(&self) {
        let entry = opcode::Read::new(
            types::Fd(self.eventfd.as_raw_fd()), 
            &self.buf as *const u8 as *mut u8, 
            8,
            )
            .build()
            .user_data(u64::MAX);
        self.register(entry).unwrap();
    }

    fn handle_eventfd(&self) {
        self.register_eventfd();
    }

    fn remove_timeout_entry(&self) {
        if !*self.has_timeout_entry.lock().unwrap().borrow() {
            return;
        }

        let entry = opcode::TimeoutRemove::new(u64::MAX - 1)
            .build()
            .user_data(u64::MAX - 2);

        self.register(entry).unwrap();
    }

    fn register_timeout_entry(&self, timeout: Option<Duration>) {
        if *self.has_timeout_entry.lock().unwrap().borrow() {
            return;
        }

        if let Some(timeout) = timeout {
            if timeout == Duration::from_secs(0) {
                return;
            }
            let timespec = self.timespec.lock().unwrap();
            *timespec.borrow_mut() = 
                Timespec::new().sec(timeout.as_secs()).nsec(timeout.subsec_nanos());
            
            let entry = opcode::Timeout::new(timespec.as_ptr() as *const Timespec)
                .build()
                .user_data(u64::MAX - 1);
            self.register(entry).unwrap();
            *self.has_timeout_entry.lock().unwrap().borrow_mut() = true;
        }
    }

    pub fn select(&self, events: &mut Vec<Event>, timeout: Option<Duration>) -> io::Result<usize> {
        events.clear();
        
        // self.remove_timeout_entry();
        // self.register_timeout_entry(timeout);

        let submitter = self.submitter.submitter();
        
        let want = match timeout {
            None => 1,
            Some(_) => 0,
        };
        submitter.submit_and_wait(want)?;


        let mut cq = unsafe{&mut *self.cq.get()}.completion();
        cq.sync(); // TODO
        for cqe in cq.into_iter() {
            let ret = cqe.result();
            let user_data = cqe.user_data();

            if user_data == u64::MAX {
                self.handle_eventfd();
                continue;
            }

            // if user_data == u64::MAX - 1 {
            //     *self.has_timeout_entry.lock().unwrap().borrow_mut() = false;
            //     continue;
            // }

            if user_data >= u64::MAX - 16 {
                continue;
            }

            let event = Event {ret: ret, user_data: cqe.user_data()};
            events.push(event);
        }

        // let mut iter = datas.drain(..);
        // loop {
        //     if sq.is_full() {
        //         match submitter.submit() {
        //             Ok(_) => (),
        //             Err(ref err) if err.raw_os_error() == Some(libc::EBUSY) => break,
        //             Err(err) => return Err(err.into()),
        //         }
        //     }
        //     sq.sync();
        // 
        //     match iter.next() {
        //         Some(sqe) => unsafe {
        //             let _ = sq.push(&sqe.into_entry());
        //         },
        //         None => break,
        //     }
        // }

        Ok(events.len())
    }

    pub fn register(&self, entry: squeue::Entry) -> io::Result<()> {
        let submitter = self.submitter.submitter();
        let mut sq = unsafe{&mut *self.sq.get()}.submission();
        // if sq.is_full() {
        //     match submitter.submit() {
        //         Ok(_) => (),
        //         Err(err) if err.raw_os_error() == Some(libc::EBUSY) => return Err(err.into()), // TODO
        //         Err(err) => return Err(err.into()),
        //     }
        // }
        unsafe {
            let _ = sq.push(&entry);
        }
        sq.sync();

        match submitter.submit() {
            Ok(_) => (),
            Err(err) if err.raw_os_error() == Some(libc::EBUSY) => return Err(err.into()), // TODO
            Err(err) => return Err(err.into()),
        }

        Ok(())    
    }

    pub fn register_batch(&self, entrys: &[squeue::Entry]) -> io::Result<()> {
        let submitter = self.submitter.submitter();
        let mut sq = unsafe{&mut *self.sq.get()}.submission();

        let mut empty = sq.capacity() - sq.len();
        let mut has_entry = false;

        let mut iter = entrys.into_iter();
        while let Some(entry) = iter.next() {
            if empty != 0 {
                unsafe { sq.push(entry); }
                empty -= 1;
                has_entry = true;
            } else {
                sq.sync();
                match submitter.submit() {
                    Ok(_) => (),
                    Err(err) if err.raw_os_error() == Some(libc::EBUSY) => return Err(err.into()), // TODO
                    Err(err) => return Err(err.into()),
                }
                has_entry = false;
                empty = sq.capacity() - sq.len();
            }
        }

        if has_entry {
            sq.sync();
            match submitter.submit() {
                Ok(_) => (),
                Err(err) if err.raw_os_error() == Some(libc::EBUSY) => return Err(err.into()), // TODO
                Err(err) => return Err(err.into()),
            }
        }

        Ok(())
    }

    pub fn notifier(&self) -> EventFD {
        self.eventfd.clone()
    }

    pub fn notify(&self) -> io::Result<()> {
        self.eventfd.write(1).unwrap();
        Ok(())
    }

    // pub fn reregister(&mut self, entry: squeue::Entry, user_data: u64) -> io::Result<()> {
    //     self.register(entry, user_data)
    // }

    fn try_drain_backlog(&mut self) -> io::Result<()> {
        let mut iter = self.backlog.drain(..);
        let submitter = self.submitter.submitter();
        // clean backlog
        let mut sq = unsafe{&mut *self.sq.get()}.submission();
        loop {
            if sq.is_full() {
                match submitter.submit() {
                    Ok(_) => (),
                    Err(ref err) if err.raw_os_error() == Some(libc::EBUSY) => break, // TODO
                    Err(err) => return Err(err.into()),
                }
            }
            sq.sync();

            match iter.next() {
                Some(sqe) => unsafe {
                    let _ = sq.push(&sqe);
                },
                None => break,
            }
        }

        core::mem::drop(iter);

        Ok(())
    }
} 
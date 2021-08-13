use std::io;
use std::os::unix::io::RawFd;
use ::io_uring::{opcode, squeue, types, IoUring, SubmissionQueue};

mod io_uring;
use self::io_uring as sys;

pub use self::io_uring::Event;

use std::collections::BTreeMap;
use std::mem;
use std::sync::{
    atomic::{AtomicUsize, Ordering, AtomicBool},
    Arc,
};
use std::task::{Poll, Waker};
use std::thread;
use std::time::{Duration, Instant};


use crate::parking;

use concurrent_queue::ConcurrentQueue;
use futures_util::future::poll_fn;
use once_cell::sync::Lazy;
use parking_lot::{Mutex, MutexGuard};
use slab::Slab;

pub struct Proactor {
    notified: AtomicBool,
    reactor: sys::Reactor,
    timer_ops: ConcurrentQueue<TimerOp>,
    timers: Mutex<BTreeMap<(Instant, usize), Waker>>,
}

pub struct Trigger {
    pub waker: Waker,
    pub ret: *mut Option<i32>,
}

pub struct Notifier {
    eventfd: eventfd::EventFD,
    notified: *const AtomicBool,
}

unsafe impl Send for Notifier {}
unsafe impl Sync for Notifier {}

impl Notifier {
    pub fn notify(&self) -> io::Result<()> {
        if unsafe{ &*self.notified }
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.eventfd.write(1).unwrap();
        }

        Ok(())
    }
}


impl Proactor {
    pub fn with<F, R>(f: F) -> R 
    where
        F: FnOnce(&Lazy<Proactor>) -> R,
    {
        thread_local! {
            static PROACTOR: Lazy<Proactor> = Lazy::new(|| Proactor::new());
        }

        PROACTOR.with(f)
    }
}

impl Proactor {
    fn new() -> Proactor {
        Proactor {
            notified: AtomicBool::new(false),
            reactor: sys::Reactor::new().expect("init reactor fail"),
            timer_ops: ConcurrentQueue::bounded(1024),
            timers: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn notifier(&self) -> Notifier {
        Notifier {
            eventfd: self.reactor.notifier(),
            notified: &self.notified,
        }
    }

    pub fn wait(&self, timeout: Option<Duration>) -> io::Result<()> {
        let mut wakers = Vec::new();

        let next_timer = self.process_timers(&mut wakers);
        let timeout = match (next_timer, timeout) {
            (None, None) => None,
            (Some(t), None) | (None, Some(t)) => Some(t),
            (Some(a), Some(b)) => Some(a.min(b)),
        };


        let mut events = vec![];
        let num_events = self.reactor.select(&mut events, timeout);
        self.notified.swap(false, Ordering::SeqCst);

        let res = match num_events {
            Ok(0) => {
                if timeout != Some(Duration::from_secs(0)) {
                    // The non-zero timeout was hit so fire ready timers.
                    self.process_timers(&mut wakers);
                }

                Ok(())
            },
            Ok(_) => {
                for ev in events.iter() {
                    let trigger = unsafe{*Box::from_raw(ev.user_data as *mut Trigger)};
                    unsafe {*trigger.ret = Some(ev.ret);}
                    wakers.push(trigger.waker);
                }
                Ok(())
            },
            // The syscall was interrupted.
            Err(err) if err.kind() == io::ErrorKind::Interrupted => Ok(()),

            // An actual error occureed.
            Err(err) => Err(err),
        };

        // Wake up ready tasks.
        for waker in wakers {
            waker.wake();
        }

        res
    }

    // pub fn remove(&self, entry: squeue::Entry) -> io::Result<()> {
    //     self.reactor.remove(fd)
    // }

    pub fn register(&self, entry: squeue::Entry) -> io::Result<()> {
        self.reactor.register(entry)
    }

    pub fn register_batch(&self, entrys: &[squeue::Entry]) -> io::Result<()>  {
        self.reactor.register_batch(entrys)
    }


    pub fn insert_timer(&self, when: Instant, waker: &Waker) -> usize {
        // Generate a new timer ID.
        static ID_GENERATOR: AtomicUsize = AtomicUsize::new(1);
        let id = ID_GENERATOR.fetch_add(1, Ordering::Relaxed);

        while self
            .timer_ops
            .push(TimerOp::Insert(when, id, waker.clone()))
            .is_err()
        {
            let mut timers = self.timers.lock();
            self.process_timer_ops(&mut timers);
        }
        self.notify();

        id
    }

    fn notify(&self) -> io::Result<()> {
        if self
            .notified
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.reactor.notify()?;
        }

        Ok(())
    }

    pub fn remove_timer(&self, when: Instant, id: usize) {
        while self.timer_ops.push(TimerOp::Remove(when, id)).is_err() {
            let mut timers = self.timers.lock();
            self.process_timer_ops(&mut timers);
        }
    }

    fn process_timers(&self, wakers: &mut Vec<Waker>) -> Option<Duration> {
        let mut timers = self.timers.lock();
        self.process_timer_ops(&mut timers);

        let now = Instant::now();

        // Split timers into ready and pending timers.
        let pending = timers.split_off(&(now, 0));

        let ready = mem::replace(&mut *timers, pending);

        let dur = if ready.is_empty() {
            timers
                .keys()
                .next()
                .map(|(when, _)| when.saturating_duration_since(now))
        } else {
            Some(Duration::from_secs(0))
        };

        drop(timers);

        for (_, waker) in ready {
            wakers.push(waker);
        }

        dur
    }

    fn process_timer_ops(&self, timers: &mut MutexGuard<'_, BTreeMap<(Instant, usize), Waker>>) {
        for _ in 0..self.timer_ops.capacity().unwrap() {
            match self.timer_ops.pop() {
                Ok(TimerOp::Insert(when, id, waker)) => {
                    timers.insert((when, id), waker);
                }
                Ok(TimerOp::Remove(when, id)) => {
                    timers.remove(&(when, id));
                }
                Err(_) => break,
            }
        }
    }
}


enum TimerOp {
    Insert(Instant, usize, Waker),
    Remove(Instant, usize),
}

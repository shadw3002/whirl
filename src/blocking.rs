use std::cell::Cell;
use std::future::Future;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use io_uring::{opcode, squeue, types, IoUring, SubmissionQueue, CompletionQueue};
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use crate::proactor::Proactor;
use crate::parking;
use crate::waker_fn::waker_fn;
use super::ebuf;


/// Runs a closure when dropped.
struct CallOnDrop<F: Fn()>(F);

impl<F: Fn()> Drop for CallOnDrop<F> {
    fn drop(&mut self) {
        (self.0)();
    }
}

/// Runs a future to completion on the current thread.
pub fn block_on<T>(future: impl Future<Output = T>) -> T {
    let waker = waker_fn({
        let notifier = Proactor::with(|p| p.notifier());
        move || {notifier.notify();}
    });
    let cx = &mut Context::from_waker(&waker);

    pin_mut!(future);

    loop {
        if let Poll::Ready(t) = future.as_mut().poll(cx) {
            return t;
        }

        ebuf::ENTRY_BUFFER.with(|buf| {
            let buf = unsafe{ &mut *buf.get() };
            let entrys = buf.drain(..).collect::<Vec<squeue::Entry>>();
            Proactor::with(|p| p.register_batch(&entrys[..])).unwrap();
        });

        let _ = Proactor::with(|p| p.wait(None));

    }
}

use super::proactor;
use super::ebuf;
use std::future::Future;
use std::io::{self, Read, Write};
use std::os::unix::io::AsRawFd;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};
use std::time::Instant;

use io_uring::{opcode, squeue, types, IoUring, SubmissionQueue, Submitter};



pub use futures_util::io::{copy, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use futures_util::io::{IoSlice, IoSliceMut};

pub struct WaitComplele {
    ret: Box<Option<i32>>,
    entry: squeue::Entry,
    first: bool,
    user_data: u64,
}

impl WaitComplele {
    fn new(entry: squeue::Entry) -> Self {
        Self {ret: Box::new(None), entry, first: true, user_data: 0}
    }
}

impl Future for WaitComplele {
    type Output = i32;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.first {
            let trigger = proactor::Trigger{waker: cx.waker().clone(), ret: &mut *self.ret as _};
            self.user_data = Box::into_raw(Box::from(trigger)) as u64;
            self.entry = self.entry.clone().user_data(self.user_data);
            // proactor::Proactor::with(|p| p.register(self.entry.clone()).unwrap());
            ebuf::ENTRY_BUFFER.with(|buf| unsafe{ &mut *buf.get() }.push(self.entry.clone()));
            self.first = false;

            return Poll::Pending;
        }

        match *self.ret {
            Some(ret) => Poll::Ready(ret),
            None => Poll::Pending,
        }
    }
}

#[derive(Debug)]
pub struct Async<T> {

    /// The inner I/O handle.
    io: Option<T>,
}

impl<T> Unpin for Async<T> {}

impl<T: AsRawFd> Async<T> {
    pub fn new(io: T) -> io::Result<Async<T>> {
        Ok(Async {
            io: Some(io),
        })
    }
}

impl<T> Async<T> {
    pub fn get_ref(&self) -> &T {
        self.io.as_ref().unwrap()
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.io.as_mut().unwrap()
    }

    pub fn into_inner(mut self) -> io::Result<T> {
        let io = self.io.take().unwrap();
        Ok(io)
    }
}

impl<T> Drop for Async<T> {
    fn drop(&mut self) {
        //
    }
}

impl<T: AsRawFd> Async<T> {
    pub async fn accept(&self) -> io::Result<usize> {
        use std::ptr;
        
        if let Some(io) = &self.io {
            let p1: *const libc::sockaddr = ptr::null();
            let p2: *const libc::socklen_t = ptr::null();
            let entry = opcode::Accept::new(
                types::Fd(io.as_raw_fd()),
                p1 as *mut libc::sockaddr,
                p2 as *mut libc::socklen_t,
            ).build();

            let ret = WaitComplele::new(entry).await;

            if ret < 0 {
                panic!("!!!")
            } else {
                Ok(ret as usize)
            }
        } else {
            panic!("!!!")
        }
    }

    pub async fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        

        let io = self.io.as_ref().unwrap();
        let raw_fd = io.as_raw_fd();
        let entry = opcode::Read::new(types::Fd(raw_fd), buf.as_mut_ptr(), buf.len() as _)
            .build();

        let ret = WaitComplele::new(entry).await;
        Ok(ret as usize)
    }

    pub async fn write(&self, buf: &[u8]) -> io::Result<usize> {
        let io = self.io.as_ref().unwrap();
        let raw_fd = io.as_raw_fd();
        let entry = opcode::Write::new(types::Fd(raw_fd), buf.as_ptr(), buf.len() as _)
            .build();
        let ret = WaitComplele::new(entry).await;
        Ok(ret as usize)
    }
}

impl<T: AsRawFd> AsyncRead for Async<T> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        poll_future(cx, async {
            if let Some(io) = &self.io {
                let raw_fd = io.as_raw_fd();
                let entry = opcode::Read::new(types::Fd(raw_fd), buf.as_mut_ptr(), buf.len() as _)
                    .build();

                let ret = WaitComplele::new(entry).await;

                Ok(ret as usize)
            } else {
                panic!("!!!")
            }
        })
    }

    fn poll_read_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
    ) -> Poll<io::Result<usize>> {
        // poll_future(cx, self.read_with_mut(|io| io.read_vectored(bufs)))
        Poll::Pending
    }
}

impl<T: AsRawFd> AsyncRead for &Async<T>
where
{
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut [u8],
    ) -> Poll<io::Result<usize>> {

        poll_future(cx, async {
            if let Some(io) = &self.io {
                let raw_fd = io.as_raw_fd();
                let entry = opcode::Read::new(types::Fd(raw_fd), buf.as_mut_ptr(), buf.len() as _)
                    .build();

                let ret = WaitComplele::new(entry).await;

                Ok(ret as usize)
            } else {
                panic!("!!!")
            }
        })
    }

    fn poll_read_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &mut [IoSliceMut<'_>],
    ) -> Poll<io::Result<usize>> {
        // poll_future(cx, self.read_with(|io| (&*io).read_vectored(bufs)))
        Poll::Pending
    }
}

impl<T: AsRawFd> AsyncWrite for Async<T> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        poll_future(cx, async {
            if let Some(io) = &self.io {
                let raw_fd = io.as_raw_fd();
                let entry = opcode::Write::new(types::Fd(raw_fd), buf.as_ptr(), buf.len() as _)
                    .build();

                let ret = WaitComplele::new(entry).await;

                Ok(ret as usize)
            } else {
                panic!("!!!")
            }
        })
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        // poll_future(cx, self.write_with_mut(|io| io.write_vectored(bufs)))
        Poll::Pending
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // poll_future(cx, self.write_with_mut(|io| io.flush()))
        Poll::Pending
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        // Poll::Ready(Ok(()))
        Poll::Pending
    }
}

impl<T: AsRawFd> AsyncWrite for &Async<T>
where
{
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        poll_future(cx, async {
            if let Some(io) = &self.io {
                let raw_fd = io.as_raw_fd();
                let entry = opcode::Write::new(types::Fd(raw_fd), buf.as_ptr(), buf.len() as _)
                    .build();

                let ret = WaitComplele::new(entry).await;

                Ok(ret as usize)
            } else {
                panic!("!!!")
            }
        })
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        // poll_future(cx, self.write_with(|io| (&*io).write_vectored(bufs)))
        Poll::Pending
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        // poll_future(cx, self.write_with(|io| (&*io).flush()))
        Poll::Pending
    }

    fn poll_close(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<io::Result<()>> {
        // Poll::Ready(Ok(()))
        Poll::Pending
    }
}

fn poll_future<T>(cx: &mut Context<'_>, fut: impl Future<Output = T>) -> Poll<T> {
    pin_mut!(fut);
    fut.poll(cx)
}

pub struct Timer {
    deadline: Instant,
    key: usize,
    waker: Option<Waker>,
}

impl Timer {
    pub fn new(deadline: Instant) -> Timer {
        Timer {
            deadline,
            key: 0,
            waker: None,
        }
    }

    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    pub fn is_elapsed(&self) -> bool {
        self.deadline < Instant::now()
    }

    pub fn reset(&mut self, when: Instant) {
        if let Some(waker) = self.waker.as_ref() {
            proactor::Proactor::with(|p| p.remove_timer(self.deadline, self.key));
            self.key = proactor::Proactor::with(|p| p.insert_timer(when, waker));            
        }

        self.deadline = when;
    }
}

impl Future for Timer {
    type Output = Instant;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.deadline <= Instant::now() {
            if self.key > 0 {
                proactor::Proactor::with(|p| p.remove_timer(self.deadline, self.key));
            }

            Poll::Ready(self.deadline)
        } else {
            match self.waker {
                None => {
                    self.key = proactor::Proactor::with(|p| p.insert_timer(self.deadline, cx.waker()));   
                    self.waker = Some(cx.waker().clone());
                }
                Some(ref w) if !w.will_wake(cx.waker()) => {
                    proactor::Proactor::with(|p| p.remove_timer(self.deadline, self.key));

                    self.key = proactor::Proactor::with(|p| p.insert_timer(self.deadline, cx.waker()));   
                    self.waker = Some(cx.waker().clone());
                }
                _ => {}
            }

            Poll::Pending
        }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        if self.waker.take().is_some() {
            proactor::Proactor::with(|p| p.remove_timer(self.deadline, self.key));
        }
    }
}

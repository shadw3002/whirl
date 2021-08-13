use std::future::Future;
use std::io;
use std::net::{self, SocketAddr, ToSocketAddrs, SocketAddrV4};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::os::unix::io::FromRawFd;

use super::stream::TcpStream;
use crate::io::Async;

use futures_util::stream::Stream;

pub struct TcpListener {
    inner: Async<net::TcpListener>,
}

impl TcpListener {
    pub async fn bind<A: ToSocketAddrs>(addr: A) -> io::Result<TcpListener> {
        let listener = net::TcpListener::bind(addr)?;

        Ok(TcpListener {
            inner: Async::new(listener)?,
        })
    }

    pub fn from_std(listener: net::TcpListener) -> io::Result<TcpListener> {
        Ok(TcpListener {
            inner: Async::new(listener)?,
        })
    }

    pub async fn accept(&self) -> io::Result<TcpStream> {
        let fd = self.inner.accept().await.unwrap();
        let raw = unsafe{net::TcpStream::from_raw_fd(fd as i32)};
        
        Ok(TcpStream::from_std(raw).unwrap())
    }

    pub fn incoming(&self) -> Incoming<'_> {
        Incoming { inner: self }
    }

    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.inner.get_ref().local_addr()
    }
}

pub struct Incoming<'a> {
    inner: &'a TcpListener,
}

impl<'a> Stream for Incoming<'a> {
    type Item = io::Result<TcpStream>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let fut = self.inner.accept();
        pin_mut!(fut);
        let stream = ready!(fut.poll(cx))?;
        Poll::Ready(Some(Ok(stream)))
    }
}

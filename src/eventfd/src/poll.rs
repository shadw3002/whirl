extern crate std;
extern crate libc;

use libc::{c_int, c_short, c_ulong, timespec};

#[repr(C)]
pub struct pollfd {
    pub fd:         c_int,
    pub events:     c_short,
    pub revents:    c_short,
}

pub const POLLIN: c_short     = 0x0001;
pub const POLLPRI: c_short    = 0x0002;
pub const POLLOUT: c_short    = 0x0004;

pub const POLLRDNORM: c_short = 0x0040;
pub const POLLRDBAND: c_short = 0x0080;
pub const POLLWRNORM: c_short = 0x0100;
pub const POLLWRBAND: c_short = 0x0200;

pub const POLLMSG: c_short    = 0x0400;
pub const POLLREMOVE: c_short = 0x1000;
pub const POLLRDHUP: c_short  = 0x2000;

pub const POLLERR: c_short    = 0x0008;
pub const POLLHUP: c_short    = 0x0008;
pub const POLLNVAL: c_short   = 0x0008;

#[cfg(target_arch = "x86_64")]
pub const sizeof_c_ulong: uint = 8;

#[cfg(target_arch = "x86")]
pub const sizeof_c_ulong: uint = 4;

#[repr(C)]
pub struct sigset {
    val: [c_ulong, ..1024 / (8 * sizeof_c_ulong)]
}

extern "C" {
    pub fn poll(fds: *mut pollfd, nfds: c_ulong, timeout: c_int) -> c_int;
    pub fn ppoll(fds: *mut pollfd, nfds: c_ulong, timeout: *const timespec, sigmask: *const sigset) -> c_int;
}

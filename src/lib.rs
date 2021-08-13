#[macro_export]
macro_rules! pin_mut {
    ($($x:ident),* $(,)?) => { $(
        // Move the value to ensure that it is owned
        let mut $x = $x;
        // Shadow the original binding so that it can't be directly accessed
        // ever again.
        #[allow(unused_mut)]
        let mut $x = unsafe {
            std::pin::Pin::new_unchecked(&mut $x)
        };
    )* }
}

#[macro_export]
macro_rules! ready {
    ($e:expr $(,)?) => {
        match $e {
            std::task::Poll::Ready(t) => t,
            std::task::Poll::Pending => return core::task::Poll::Pending,
        }
    };
}

pub mod blocking;
pub mod runtime;

pub mod net;
pub mod time;

mod parking;
mod waker_fn;
mod io;
mod proactor;
mod ebuf;


use std::future::Future;
use std::thread;

pub use async_task::Task;
pub use blocking::block_on;
pub use runtime::Runtime;

pub use futures_util::stream::StreamExt;

use once_cell::sync::Lazy;

pub static RUNTIME: Lazy<Runtime> = Lazy::new(|| {
    for i in 0..num_cpus::get().max(1) {
        thread::spawn(move || {
            let worker = RUNTIME.worker(i);
            block_on(worker.run());
        });
    }

    Runtime::new()
});

pub fn spawn<T: Send + 'static>(future: impl Future<Output = T> + Send + 'static) -> Task<T> {
    RUNTIME.spawn(future)
}

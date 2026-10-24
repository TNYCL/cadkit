//! Minimal executor for the futures of this crate on native `wgpu`.

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

/// Runs `future` to completion on the current thread by polling it in a yielding loop.
///
/// Native `wgpu` resolves error scopes and buffer mappings during `Device::poll`, which
/// this crate's futures call themselves, so they complete without a reactor. Do not use it
/// for futures that wait on other threads or on the browser event loop (WebGPU).
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut context = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
            return output;
        }
        std::thread::yield_now();
    }
}

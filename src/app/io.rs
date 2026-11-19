//! Disk work uses one worker, with admission occurring when each future is polled.
//! If worker creation fails, work runs inline when the future is polled.

use enzui_core::blocking_task::{BlockingTaskError, BlockingTaskOwner};
use std::future::Future;
use std::num::NonZeroUsize;
use std::sync::OnceLock;

const QUEUE: NonZeroUsize = NonZeroUsize::new(64).expect("disk queue capacity is nonzero");

pub(crate) fn disk<T, W>(work: W) -> impl Future<Output = T> + Send + 'static
where
    T: Send + 'static,
    W: FnOnce() -> T + Send + 'static,
{
    let localizer = enzui_core::localization::current();
    let work = move || localizer.scope(work);
    static OWNER: OnceLock<Option<BlockingTaskOwner>> = OnceLock::new();
    let owner = OWNER
        .get_or_init(|| BlockingTaskOwner::new("video-disk", NonZeroUsize::MIN, QUEUE).ok())
        .as_ref();
    enum Route<W, J> {
        Inline(W),
        Owned(J),
    }
    let route = match owner {
        Some(owner) => Route::Owned(owner.submit_waiting(0, move |_| work())),
        None => Route::Inline(work),
    };
    async move {
        match route {
            Route::Inline(work) => work(),
            Route::Owned(job) => match job.await.into_result() {
                Ok(value) => value,
                Err(BlockingTaskError::Panicked) => disk_worker_panicked(),
                // Closed ownership must not deliver into the shutting-down application.
                Err(_) => std::future::pending().await,
            },
        }
    }
}

#[expect(
    clippy::panic,
    reason = "A panicked worker cannot supply T; propagate the failure instead of hanging its caller"
)]
fn disk_worker_panicked() -> ! {
    panic!("video disk worker panicked")
}

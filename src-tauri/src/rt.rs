//! Spawning app-lifetime tasks without caring which build this is.
//!
//! The desktop build boots inside Tauri's `setup` hook, which is NOT running on
//! a tokio worker, so a bare `tokio::spawn` there would panic ("no reactor
//! running"). Tauri's global runtime is the right home for those tasks. The
//! headless daemon (`daemon.rs`) owns its own tokio runtime and every caller is
//! already inside it, so a plain `tokio::spawn` is correct.

use std::future::Future;

/// Spawn a detached, app-lifetime task on the app's runtime.
pub fn spawn<F>(future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    #[cfg(feature = "gui")]
    {
        tauri::async_runtime::spawn(future);
    }
    #[cfg(not(feature = "gui"))]
    {
        tokio::spawn(future);
    }
}

/// Run `future` to completion on the app's runtime from synchronous code (the
/// daemon entrypoint). In the desktop build that is Tauri's global runtime, so
/// [`spawn`] and the caller share one reactor; headless builds a fresh
/// multi-threaded tokio runtime.
pub fn block_on<F>(future: F) -> std::io::Result<F::Output>
where
    F: Future,
{
    #[cfg(feature = "gui")]
    {
        Ok(tauri::async_runtime::block_on(future))
    }
    #[cfg(not(feature = "gui"))]
    {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()?;
        Ok(runtime.block_on(future))
    }
}

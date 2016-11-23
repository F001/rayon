use latch::LockLatch;
#[allow(unused_imports)]
use log::Event::*;
use job::StackJob;
use std::sync::Arc;
use std::error::Error;
use std::fmt;
use thread_pool::{self, Registry};

/// Custom error type for the rayon thread pool configuration.
#[derive(Debug,PartialEq)]
pub enum InitError {
    /// Error if number of threads is set to zero.
    NumberOfThreadsZero,

    /// Error if the gloal thread pool is initialized multiple times
    /// and the configuration is not equal for all configurations.
    GlobalPoolAlreadyInitialized,
}

impl fmt::Display for InitError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            InitError::NumberOfThreadsZero => {
                write!(f,
                       "The number of threads was set to zero but must be greater than zero.")
            }
            InitError::GlobalPoolAlreadyInitialized => {
                write!(f,
                       "The gobal thread pool has already been initialized with a different \
                        configuration. Only one valid configuration is allowed.")
            }
        }
    }
}

impl Error for InitError {
    fn description(&self) -> &str {
        match *self {
            InitError::NumberOfThreadsZero => "number of threads set to zero",
            InitError::GlobalPoolAlreadyInitialized => {
                "global thread pool has already been initialized"
            }
        }
    }
}

/// Contains the rayon thread pool configuration.
#[derive(Clone, Debug)]
pub struct Configuration {
    /// The number of threads in the rayon thread pool. Must not be zero.
    num_threads: Option<usize>,
}

impl Configuration {
    /// Creates and return a valid rayon thread pool configuration, but does not initialize it.
    pub fn new() -> Configuration {
        Configuration { num_threads: None }
    }

    /// Get the number of threads that will be used for the thread
    /// pool. See `set_num_threads` for more information.
    pub fn num_threads(&self) -> Option<usize> {
        self.num_threads
    }

    /// Set the number of threads to be used in the rayon threadpool.
    /// The argument `num_threads` must not be zero. If you do not
    /// call this function, rayon will select a suitable default
    /// (currently, the default is one thread per CPU core).
    pub fn set_num_threads(mut self, num_threads: usize) -> Configuration {
        self.num_threads = Some(num_threads);
        self
    }

    /// Checks whether the configuration is valid.
    fn validate(&self) -> Result<(), InitError> {
        if let Some(value) = self.num_threads {
            if value == 0 {
                return Err(InitError::NumberOfThreadsZero);
            }
        }

        Ok(())
    }
}

/// Initializes the global thread pool. This initialization is
/// **optional**.  If you do not call this function, the thread pool
/// will be automatically initialized with the default
/// configuration. In fact, calling `initialize` is not recommended,
/// except for in two scenarios:
///
/// - You wish to change the default configuration.
/// - You are running a benchmark, in which case initializing may
///   yield slightly more consistent results, since the worker threads
///   will already be ready to go even in the first iteration.  But
///   this cost is minimal.
///
/// Initialization of the global thread pool happens exactly
/// once. Once started, the configuration cannot be
/// changed. Therefore, if you call `initialize` a second time, it
/// will simply check that the global thread pool already has the
/// configuration you requested, rather than making changes.
///
/// An `Ok` result indicates that the thread pool is running with the
/// given configuration. Otherwise, a suitable error is returned.
pub fn initialize(config: Configuration) -> Result<(), InitError> {
    try!(config.validate());

    let num_threads = config.num_threads;

    let registry = thread_pool::get_registry_with_config(config);

    if let Some(value) = num_threads {
        if value != registry.num_threads() {
            return Err(InitError::GlobalPoolAlreadyInitialized);
        }
    }

    registry.wait_until_primed();

    Ok(())
}

/// This is a debugging API not really intended for end users. It will
/// dump some performance statistics out using `println`.
pub fn dump_stats() {
    dump_stats!();
}

pub struct ThreadPool {
    registry: Arc<Registry>,
}

impl ThreadPool {
    /// Constructs a new thread pool with the given configuration. If
    /// the configuration is not valid, returns a suitable `Err`
    /// result.  See `InitError` for more details.
    pub fn new(configuration: Configuration) -> Result<ThreadPool, InitError> {
        try!(configuration.validate());
        Ok(ThreadPool { registry: Registry::new(configuration.num_threads) })
    }

    /// Executes `op` within the threadpool. Any attempts to `join`
    /// which occur there will then operate within that threadpool.
    pub fn install<OP, R>(&self, op: OP) -> R
        where OP: FnOnce() -> R + Send
    {
        unsafe {
            let job_a = StackJob::new(op, LockLatch::new());
            self.registry.inject(&[job_a.as_job_ref()]);
            job_a.latch.wait();
            job_a.into_result()
        }
    }
}

impl Drop for ThreadPool {
    fn drop(&mut self) {
        self.registry.terminate();
    }
}

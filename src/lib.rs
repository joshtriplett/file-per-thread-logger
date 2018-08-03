#[macro_use]
extern crate log;
extern crate env_logger;

use std::cell::RefCell;
use std::env;
use std::fs::File;
use std::io::{self, Write};
use std::sync::atomic;
use std::thread;

use env_logger::filter::{Builder, Filter};
use log::{LevelFilter, Metadata, Record};

static INITIALIZED: atomic::AtomicBool = atomic::AtomicBool::new(false);

thread_local! {
    static WRITER: RefCell<Option<io::BufWriter<File>>> = RefCell::new(None);
}

/// Initializes the current process/thread with a logger, parsing the RUST_LOG environment
/// variables to set the logging level filter and/or directives to set a filter by module name,
/// following the usual env_logger conventions.
///
/// Must be called on every running thread, or else logging will panic the first time it's used.
pub fn initialize(filename_prefix: &str) {
    let level_filter = env::var_os("RUST_LOG").map(|val| {
        let mut builder = Builder::new();
        builder.parse(&val.to_str().unwrap());
        builder.build()
    });

    if level_filter.is_some() {
        // Ensure the thread local state is always properly initialized.
        WRITER.with(|rc| {
            if rc.borrow().is_none() {
                rc.replace(Some(open_file(filename_prefix)));
            }
        });
    }

    if INITIALIZED.load(atomic::Ordering::Relaxed) || level_filter.is_none() {
        return;
    }

    INITIALIZED.store(true, atomic::Ordering::Relaxed);

    let logger = FilePerThreadLogger::new(level_filter.unwrap());
    let setup_result =
        log::set_boxed_logger(Box::new(logger)).map(|()| log::set_max_level(LevelFilter::max()));
    match setup_result {
        Ok(_) => {}
        Err(_) => {
            warn!("Another logger has been set up before the file-per-thread logger, aborting.");
        }
    }

    info!("Set up logging; filename prefix is {}", filename_prefix);
}

struct FilePerThreadLogger {
    filter: Filter,
}

impl FilePerThreadLogger {
    pub fn new(filter: Filter) -> Self {
        FilePerThreadLogger { filter }
    }
}

impl log::Log for FilePerThreadLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        enabled() && self.filter.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        if self.enabled(record.metadata()) {
            WRITER.with(|rc| {
                let mut opt_writer = rc.borrow_mut();
                let writer = opt_writer
                    .as_mut()
                    .expect("call the logger's initialize() function first");
                let _ = writeln!(*writer, "{} - {}", record.level(), record.args());
            })
        }
    }

    fn flush(&self) {
        WRITER.with(|rc| {
            let mut opt_writer = rc.borrow_mut();
            let writer = opt_writer
                .as_mut()
                .expect("call the logger's initialize() function first");
            let _ = writer.flush();
        });
    }
}

/// Checks whether the logging state has ever been initialized or not.
#[inline]
fn enabled() -> bool {
    INITIALIZED.load(atomic::Ordering::Relaxed)
}

/// Open the tracing file for the current thread.
fn open_file(filename_prefix: &str) -> io::BufWriter<File> {
    let curthread = thread::current();
    let tmpstr;
    let mut path = filename_prefix.to_owned();
    path.extend(
        match curthread.name() {
            Some(name) => name.chars(),
            // The thread is unnamed, so use the thread ID instead.
            None => {
                tmpstr = format!("{:?}", curthread.id());
                tmpstr.chars()
            }
        }.filter(|ch| ch.is_alphanumeric() || *ch == '-' || *ch == '_'),
    );
    let file = File::create(path).expect("Can't open tracing file");
    io::BufWriter::new(file)
}

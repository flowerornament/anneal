//! Test-only helpers for rendering `Printer` output into a buffer that
//! the caller can still read back. `Printer` owns a `Box<dyn Write +
//! 'static>`, so tests can't hand it a `&mut Vec<u8>` directly; a
//! shared `Rc<RefCell<Vec<u8>>>` threads the bytes out after the
//! printer is dropped.

use std::cell::RefCell;
use std::io;
use std::rc::Rc;

/// Shared buffer so tests can construct a `Printer` (which owns its
/// writer) and still read the written bytes back.
pub(crate) struct SharedBuf(Rc<RefCell<Vec<u8>>>);

impl SharedBuf {
    pub(crate) fn new() -> (Self, Rc<RefCell<Vec<u8>>>) {
        let buf = Rc::new(RefCell::new(Vec::new()));
        (Self(Rc::clone(&buf)), buf)
    }
}

impl io::Write for SharedBuf {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        self.0.borrow_mut().write(b)
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

pub use core::fmt;
pub use core::fmt::Write;
/// Re-export the bits we use so that code generated by the
/// macros is guaranteed to be able to find them.
pub use userlib::util::StaticCell;

/// Declares a ringbuffer in the current module or context.
///
/// `stringbuf!(NAME, N, expr)` makes a ringbuffer named `NAME`,
/// with room for `N` such entries, all of which are initialized to `expr`.
///
/// The resulting ringbuffer will be static, so `NAME` should be uppercase. If
/// you want your ringbuffer to be detected by Humility's automatic scan, its
/// name should end in `STRINGBUF`.
///
/// The actual type of `name` will be `StaticCell<Stringbuf<N>>`.
///
/// To support the common case of having one quickly-installed ringbuffer per
/// module, if you omit the name, it will default to `LOG__STRINGBUF`.
#[cfg(not(feature = "disabled"))]
#[macro_export]
macro_rules! stringbuf {
    ($name:ident, $n:expr, $init:expr) => {
        #[used]
        pub static $name: $crate::StaticCell<$crate::stringbuf::Stringbuf<$n>> =
            $crate::StaticCell::new($crate::stringbuf::Stringbuf {
                last: None,
                buffer: [0; $n],
            });
    };
    ($n:expr, $init:expr) => {
        $crate::stringbuf!(LOG__STRINGBUF, $n, $init);
    };
}

#[cfg(feature = "disabled")]
#[macro_export]
macro_rules! stringbuf {
    ($name:ident, $n:expr, $init:expr) => {
        #[allow(dead_code)]
        const _: u8 = $init;
    };
    ($n:expr, $init:expr) => {
        #[allow(dead_code)]
        const _: u8 = $init;
    };
}

/// Inserts data into a named ringbuffer (which should have been declared with
/// the `stringbuf!` macro).
///
/// `stringbuf_entry!(NAME, expr)` will insert `expr` into the ringbuffer called
/// `NAME`.
///
/// If you declared your ringbuffer without a name, you can also use this
/// without a name, and it will default to `LOG__STRINGBUF`.
#[cfg(not(feature = "disabled"))]
#[macro_export]
macro_rules! stringbuf_entry {
    ($buf:expr, $payload:expr) => {{
        let mut buf = &mut *$crate::StaticCell::borrow_mut(&$buf);
        buf.write_fmt($payload).unwrap();
    }};
    ($payload:expr) => {
        $crate::stringbuf_entry!(LOG__STRINGBUF, $payload);
    };
}

#[cfg(feature = "disabled")]
#[macro_export]
macro_rules! stringbuf_entry {
    ($buf:expr, $payload:expr) => {{
        let _ = &$buf;
        let _ = &$payload;
    }};
    ($payload:expr) => {{
        let _ = &$payload;
    }};
}

/// Inserts data into an unnamed ringbuffer at the root of this crate
#[cfg(not(feature = "disabled"))]
#[macro_export]
macro_rules! stringbuf_entry_root {
    ($payload:expr) => {
        $crate::stringbuf_entry!($crate::stringbuf::LOG__STRINGBUF, $payload);
    };
}

#[cfg(feature = "disabled")]
#[macro_export]
macro_rules! stringbuf_entry_root {
    ($payload:expr) => {{
        let _ = &$payload;
    }};
}

stringbuf!(LOG__STRINGBUF, 128, 0);

///
/// A ring buffer of parametrized size.  In practice, instantiating
/// this directly is strange -- see the [`stringbuf!`] macro.
///
#[derive(Debug)]
pub struct Stringbuf<const N: usize> {
    pub last: Option<usize>,
    pub buffer: [u8; N],
}

///
/// Implementing fmt::Write allows string formatting without an allocator
///
impl<const N: usize> fmt::Write for Stringbuf<{ N }> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for c in s.as_bytes() {
            self.entry(*c);
        }
        Ok(())
    }
}

impl<const N: usize> Stringbuf<{ N }> {
    pub fn entry(&mut self, payload: u8) {
        let ndx = match self.last {
            None => 0,
            Some(last) => {
                if last + 1 >= self.buffer.len() {
                    0
                } else {
                    last + 1
                }
            }
        };

        self.buffer[ndx] = payload;

        self.last = Some(ndx);
    }
}

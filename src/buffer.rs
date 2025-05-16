#[cfg(feature = "alloc")]
use alloc::{boxed::Box, vec::Vec};
use core::ops::{Deref, DerefMut};

// a representation of a buffer which can be either owned or borrowed
// normally this will just be a dynamically allocated Box<[u8]> but for no_std / no_alloc
// builds we can use a borrowed slice.
#[derive(Debug)]
pub enum Buffer<'a> {
    #[cfg(feature = "alloc")]
    Owned(Box<[u8]>),
    Borrowed(&'a mut [u8]),
}

impl Deref for Buffer<'_> {
    type Target = [u8];
    fn deref(&self) -> &Self::Target {
        match self {
            #[cfg(feature = "alloc")]
            Buffer::Owned(v) => v.as_ref(),
            Buffer::Borrowed(b) => *b,
        }
    }
}

impl DerefMut for Buffer<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            #[cfg(feature = "alloc")]
            Buffer::Owned(v) => v.as_mut(),
            Buffer::Borrowed(b) => *b,
        }
    }
}
#[cfg(feature = "alloc")]
impl From<Vec<u8>> for Buffer<'static> {
    fn from(v: Vec<u8>) -> Self {
        Buffer::Owned(v.into_boxed_slice())
    }
}

#[cfg(feature = "alloc")]
impl From<Box<[u8]>> for Buffer<'static> {
    fn from(b: Box<[u8]>) -> Self {
        Buffer::Owned(b)
    }
}

impl<'a> From<&'a mut [u8]> for Buffer<'a> {
    fn from(v: &'a mut [u8]) -> Self {
        Buffer::Borrowed(v)
    }
}

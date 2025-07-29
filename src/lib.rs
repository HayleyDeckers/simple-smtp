// no_std unless the std flag is set
#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "alloc")]
extern crate alloc;

mod error;
pub use error::*;

mod buffer;
pub use buffer::Buffer;

pub mod smtp;
pub use smtp::Smtp;

pub mod integrations {
    #[cfg(feature = "embassy")]
    mod embassy;
    #[cfg(feature = "embassy")]
    pub use embassy::EmbassyTcpError;
    #[cfg(feature = "lettre")]
    mod lettre;
    #[cfg(feature = "tokio")]
    pub mod tokio;
}

pub trait ReadWrite {
    type Error: core::error::Error;
    fn read(&mut self, buf: &mut [u8]) -> impl Future<Output = Result<usize, Self::Error>>;
    fn write_single(&mut self, buf: &[u8]) -> impl Future<Output = Result<(), Self::Error>>;
    fn write_multi(&mut self, buf: &[&[u8]]) -> impl Future<Output = Result<(), Self::Error>> {
        async move {
            for b in buf {
                self.write_single(b).await?;
            }
            Ok(())
        }
    }
}

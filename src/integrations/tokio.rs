use core::ops::{Deref, DerefMut};
use std::{array, io::IoSlice};

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::ReadWrite;

pub struct TokioIo<T: AsyncRead + AsyncWrite + Unpin + Send>(pub T);
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Deref for TokioIo<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> DerefMut for TokioIo<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T: AsyncRead + AsyncWrite + Unpin + Send> ReadWrite for TokioIo<T> {
    type Error = tokio::io::Error;
    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.0.read(buf).await
    }

    async fn write_single(&mut self, buf: &[u8]) -> Result<(), Self::Error> {
        if buf.is_empty() {
            return Ok(());
        }
        self.0.write_all(buf).await?;
        Ok(())
    }

    async fn write_multi(&mut self, buf: &[&[u8]]) -> Result<(), Self::Error> {
        if !self.is_write_vectored() || buf.len() == 1 {
            for b in buf {
                self.write_single(b).await?;
            }
            Ok(())
        } else {
            // should be enough for all cases but just to be sure
            const PER_LOOP: usize = 6;
            let loops = buf.len().div_ceil(PER_LOOP);
            for l in 0..loops {
                let mut slices: [IoSlice<'_>; PER_LOOP] = array::from_fn(|i| {
                    let idx = l * PER_LOOP + i;
                    if idx < buf.len() {
                        IoSlice::new(buf[idx])
                    } else {
                        IoSlice::new(&[])
                    }
                });
                let mut slices = {
                    let last_used_slice = {
                        let mut idx = 0;
                        for s in slices {
                            if s.is_empty() {
                                break;
                            }
                            idx += 1;
                        }
                        idx
                    };
                    &mut slices[..last_used_slice]
                };
                loop {
                    let written = self.0.write_vectored(slices).await?;
                    std::io::IoSlice::advance_slices(&mut slices, written);
                    if slices.is_empty() {
                        break;
                    }
                }
            }
            Ok(())
        }
    }
}

#[cfg(feature = "rustls")]
mod rustls_support {
    use std::sync::Arc;

    use tokio::io::{AsyncRead, AsyncWrite};
    use tokio_rustls::{TlsConnector, client::TlsStream};

    use super::TokioIo;
    use crate::{Error, ReadWrite, Smtp};
    impl<'buffer, T: AsyncRead + AsyncWrite + Unpin + Send> Smtp<'buffer, TokioIo<T>> {
        pub async fn upgrade_to_tls(
            self,
            domain: &str,
        ) -> Result<Smtp<'buffer, TokioIo<TlsStream<T>>>, Error<<TokioIo<T> as ReadWrite>::Error>>
        {
            let (tcp, buffer) = self.into_inner();

            let root_cert_store =
                rustls::RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
            let config = rustls::ClientConfig::builder()
                .with_root_certificates(root_cert_store)
                .with_no_client_auth(); // i guess this was previously the default?
            let connector = TlsConnector::from(Arc::new(config));
            let tls = connector
                .connect(
                    rustls::pki_types::ServerName::try_from(domain)
                        .unwrap()
                        .to_owned(),
                    tcp.0,
                )
                .await
                .expect("failed to connect");
            Ok(Smtp::new_with_buffer(TokioIo(tls), buffer))
        }
    }
}

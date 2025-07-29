use crate::ReadWrite;
use embassy_net::tcp::TcpSocket;

impl ReadWrite for TcpSocket<'_> {
    type Error = EmbassyTcpError;

    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.read(buf).await.map_err(EmbassyTcpError)
    }

    async fn write_single(&mut self, mut buf: &[u8]) -> Result<(), Self::Error> {
        while !buf.is_empty() {
            buf = &buf[self.write(buf).await.map_err(EmbassyTcpError)?..];
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct EmbassyTcpError(pub embassy_net::tcp::Error);

impl core::fmt::Display for EmbassyTcpError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "Embassy TCP Error")
    }
}

impl core::error::Error for EmbassyTcpError {
    fn source(&self) -> Option<&(dyn core::error::Error + 'static)> {
        None
    }
}

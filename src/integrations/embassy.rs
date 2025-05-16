use crate::ReadWrite;
use embassy_net::tcp::TcpSocket;

impl ReadWrite for TcpSocket<'_> {
    type Error = embassy_net::tcp::Error;

    async fn read(&mut self, buf: &mut [u8]) -> Result<usize, Self::Error> {
        self.read(buf).await
    }

    async fn write_single(&mut self, mut buf: &[u8]) -> Result<(), Self::Error> {
        while !buf.is_empty() {
            buf = &buf[self.write(buf).await?..]
        }
        Ok(())
    }
}

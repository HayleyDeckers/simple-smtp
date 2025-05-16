use crate::{ReadWrite, Smtp};
impl<'buf, T: ReadWrite<Error = impl core::error::Error>> Smtp<'buf, T> {
    pub async fn send_lettre(
        &mut self,
        email: lettre::Message,
    ) -> Result<(), crate::Error<T::Error>> {
        let to = email.envelope().to();
        let from = email.envelope().from().unwrap();
        let data = email.formatted();
        self.send_mail(from, to.iter(), &data).await
    }
}

// sends an to all necessary reciptients.
// It will use the to adressess on the envelope to determine the mx records.
// group all the recipients by their mx records, and establish a connection for each unique mx record.
// It will then send the email to all recipients on that mx record, by connecting on port 25.
// and using STARTTLS to upgrade the connection to TLS.
//
// Using this (properly!) will require some set-up on the server
// - you need to have outbound port 25 open
// - you need to have an SPF record pointing at your ip for the sending domain
// - you need to have a DKIM record for the sending domain (corresponding to the key used)
// - Ideally you have a DMARC record for the sending domain
// - Ideally you have a PTR record for the sending domain
//
//  for a closed network or if you're only sending to a server you control you might need these
//  but if you're sending mail to the big provides these are heavilly recommended.
// pub async fn send_email(email: lettre::Message) -> crate::Result<()> {
//     use crate::resolver;
//     let from = email
//         .envelope()
//         .from()
//         .ok_or(crate::Error::ProtocolError(crate::ProtocolError::NoSender))?;
//     let to = email.envelope().to();
//     let to_domains = to.iter().map(|to| to.domain()).collect::<HashSet<_>>();
//     // in theory multiple domains could resolve to the same mx record
//     // and we could send to all of them in one go
//     for domain in to_domains {
//         // smtp.send_lettre(email.clone()).await?;
//         let (host, ip) = resolver::lookup_mx_records(domain).await?;
//         let tcp = tokio::net::TcpStream::connect((ip, 25)).await?;
//         let mut smtp = Smtp::new(tcp);
//         smtp.ready().await?;
//         let ehlo = smtp.ehlo(from.domain()).await?;
//         if !ehlo.supports(crate::smtp::Extensions::StartTls) {
//             //todo: partial success?
//             return Err(crate::Error::ProtocolError(
//                 crate::ProtocolError::UnsupportedExtension(crate::smtp::Extensions::StartTls),
//             ));
//         }
//         // send the STARTTLS command
//         smtp.starttls().await?;
//         // domain or host?
//         let mut smtp = smtp.upgrade_to_tls(&host).await?;
//         smtp.ehlo(from.domain()).await?;
//         smtp.send_lettre(email.clone()).await?;
//         smtp.quit().await?;
//     }
//     Ok(())
// }

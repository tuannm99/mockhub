//! Chuyển câu hỏi lên một resolver thật: UDP, bị cắt (TC) thì hỏi lại qua TCP.
use std::{io, sync::Arc, time::Duration};

use hickory_proto::{
    op::{Message, MessageType, OpCode, Query, ResponseCode},
    rr::{Record, RecordType},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpStream, UdpSocket},
};

use crate::records::wire_name;

const TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone)]
pub struct Upstream(Arc<str>);

impl Upstream {
    /// `ip`, `ip:port`, `host:port` hoặc `[ipv6]:port`; thiếu cổng thì dùng 53.
    pub fn new(address: &str) -> Option<Self> {
        let address = address.trim();
        if address.is_empty() {
            return None;
        }
        if let Ok(ip) = address.parse::<std::net::IpAddr>() {
            return Some(Self(std::net::SocketAddr::new(ip, 53).to_string().into()));
        }
        let with_port = if address.contains(':') {
            address.to_owned()
        } else {
            format!("{address}:53")
        };
        Some(Self(with_port.into()))
    }

    pub fn address(&self) -> &str {
        &self.0
    }

    pub async fn query(
        &self,
        name: &str,
        qtype: RecordType,
    ) -> io::Result<(ResponseCode, Vec<Record>)> {
        tokio::time::timeout(TIMEOUT, self.exchange(name, qtype))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "upstream did not answer"))?
    }

    async fn exchange(
        &self,
        name: &str,
        qtype: RecordType,
    ) -> io::Result<(ResponseCode, Vec<Record>)> {
        let random = uuid::Uuid::new_v4().into_bytes();
        let id = u16::from_be_bytes([random[0], random[1]]);
        let mut question = Message::new(id, MessageType::Query, OpCode::Query);
        question.metadata.recursion_desired = true;
        let name = wire_name(name).map_err(io::Error::other)?;
        question.add_query(Query::query(name, qtype));
        let packet = question.to_vec().map_err(io::Error::other)?;

        let target = tokio::net::lookup_host(self.address())
            .await?
            .next()
            .ok_or_else(|| io::Error::other("upstream address resolves to nothing"))?;
        let local = if target.is_ipv4() {
            "0.0.0.0:0"
        } else {
            "[::]:0"
        };
        let socket = UdpSocket::bind(local).await?;
        // `connect`: chỉ nhận gói đến từ đúng resolver đã hỏi.
        socket.connect(target).await?;
        socket.send(&packet).await?;
        let mut buf = vec![0u8; 4096];
        let response = loop {
            let len = socket.recv(&mut buf).await?;
            match Message::from_vec(&buf[..len]) {
                Ok(m)
                    if m.metadata.id == id && m.metadata.message_type == MessageType::Response =>
                {
                    break m;
                }
                _ => continue,
            }
        };
        let response = if response.metadata.truncation {
            let mut stream = TcpStream::connect(target).await?;
            let len = u16::try_from(packet.len()).map_err(io::Error::other)?;
            stream.write_u16(len).await?;
            stream.write_all(&packet).await?;
            let mut reply = vec![0u8; usize::from(stream.read_u16().await?)];
            stream.read_exact(&mut reply).await?;
            let reply = Message::from_vec(&reply).map_err(io::Error::other)?;
            if reply.metadata.id != id {
                return Err(io::Error::other("upstream answered another question"));
            }
            reply
        } else {
            response
        };
        Ok((response.metadata.response_code, response.answers))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_port_defaults_to_53() {
        let address = |a| Upstream::new(a).map(|u| u.address().to_owned());
        assert_eq!(address("1.1.1.1").as_deref(), Some("1.1.1.1:53"));
        assert_eq!(address("1.1.1.1:5300").as_deref(), Some("1.1.1.1:5300"));
        assert_eq!(address("::1").as_deref(), Some("[::1]:53"));
        assert_eq!(address("dns.test").as_deref(), Some("dns.test:53"));
        assert_eq!(address("  "), None);
    }
}

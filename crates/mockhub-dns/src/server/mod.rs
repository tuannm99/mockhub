//! Lớp mạng: DNS cổ điển trên UDP và TCP.
use std::{net::SocketAddr, sync::Arc, time::Duration};

use hickory_proto::op::{Message, MessageType, ResponseCode};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UdpSocket},
};

use crate::{Engine, Outcome, Source};

/// Kết nối TCP im lặng quá lâu thì đóng.
const TCP_IDLE: Duration = Duration::from_secs(30);
/// Cỡ gói UDP khi client không khai EDNS (RFC 1035).
const CLASSIC_UDP: usize = 512;

/// Trả lời một gói hỏi đã parse (dùng chung cho UDP, TCP và DoH RFC 8484).
/// `None` = không gửi gì (không phải câu hỏi, hoặc fault `timeout`).
pub async fn answer_message(engine: &Engine, proto: &str, query: &Message) -> Option<Message> {
    if query.metadata.message_type != MessageType::Query {
        return None;
    }
    let mut response = Message::response(query.metadata.id, query.metadata.op_code);
    response.metadata.recursion_desired = query.metadata.recursion_desired;
    response.metadata.recursion_available = true;
    response.edns = query.edns.clone();
    let [question] = query.queries.as_slice() else {
        response.metadata.response_code = ResponseCode::FormErr;
        return Some(response);
    };
    response.add_query(question.clone());
    let name = question.name.to_ascii();
    match engine.lookup(proto, &name, question.query_type).await {
        Outcome::Drop => return None,
        Outcome::Reply(answer) => {
            response.metadata.authoritative = answer.source == Source::Mock;
            response.metadata.response_code = answer.code;
            response.add_answers(answer.records);
        }
    }
    Some(response)
}

/// Gói trả lời cùng cỡ UDP tối đa mà client nhận được.
async fn reply(engine: &Engine, proto: &str, packet: &[u8]) -> Option<(Message, usize)> {
    let query = Message::from_vec(packet).ok()?;
    let max_udp = query
        .edns
        .as_ref()
        .map_or(CLASSIC_UDP, |_| usize::from(query.max_payload()));
    Some((answer_message(engine, proto, &query).await?, max_udp))
}

pub async fn serve_udp(socket: UdpSocket, engine: Engine) -> std::io::Result<()> {
    let socket = Arc::new(socket);
    let mut buf = vec![0u8; 4096];
    loop {
        // Lỗi nhận một gói (vd ICMP port unreachable) không được làm chết service.
        let (len, peer): (usize, SocketAddr) = match socket.recv_from(&mut buf).await {
            Ok(got) => got,
            Err(e) => {
                tracing::warn!("dns udp recv: {e}");
                continue;
            }
        };
        let (socket, engine, packet) = (socket.clone(), engine.clone(), buf[..len].to_vec());
        // Mỗi gói một task: fault `delay` không được chặn các câu hỏi khác.
        tokio::spawn(async move {
            let Some((response, max)) = reply(&engine, "udp", &packet).await else {
                return;
            };
            let encoded = match response.to_vec() {
                // Quá cỡ: bật TC để client hỏi lại bằng TCP.
                Ok(bytes) if bytes.len() > max => response.truncate().to_vec(),
                other => other,
            };
            match encoded {
                Ok(bytes) => {
                    if let Err(e) = socket.send_to(&bytes, peer).await {
                        tracing::warn!("dns udp send to {peer}: {e}");
                    }
                }
                Err(e) => tracing::warn!("dns encode: {e}"),
            }
        });
    }
}

async fn serve_connection(mut stream: TcpStream, engine: Engine) -> std::io::Result<()> {
    loop {
        let Ok(len) = tokio::time::timeout(TCP_IDLE, stream.read_u16()).await else {
            return Ok(());
        };
        let mut packet = vec![0u8; usize::from(len?)];
        match tokio::time::timeout(TCP_IDLE, stream.read_exact(&mut packet)).await {
            Ok(read) => read?,
            Err(_) => return Ok(()),
        };
        let Some((response, _)) = reply(&engine, "tcp", &packet).await else {
            continue;
        };
        let Ok(bytes) = response.to_vec() else {
            return Ok(());
        };
        let Ok(len) = u16::try_from(bytes.len()) else {
            return Ok(());
        };
        stream.write_u16(len).await?;
        stream.write_all(&bytes).await?;
    }
}

pub async fn serve_tcp(listener: TcpListener, engine: Engine) -> std::io::Result<()> {
    loop {
        let (stream, peer) = match listener.accept().await {
            Ok(got) => got,
            Err(e) => {
                tracing::warn!("dns tcp accept: {e}");
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        let engine = engine.clone();
        tokio::spawn(async move {
            if let Err(e) = serve_connection(stream, engine).await {
                tracing::debug!("dns tcp {peer}: {e}");
            }
        });
    }
}

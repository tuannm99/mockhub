//! Gói DNS thật qua socket thật, trên cổng do hệ điều hành chọn.
use std::{str::FromStr, time::Duration};

use hickory_proto::{
    op::{Message, MessageType, OpCode, Query, ResponseCode},
    rr::{Name, RecordType},
};
use mockhub_core::domain::dns::{FaultAction, NewDnsFault, NewDnsRecord};
use mockhub_dns::{
    Engine, Outcome, Source,
    server::{serve_tcp, serve_udp},
};
use mockhub_storage::Storage;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream, UdpSocket},
};

fn query(id: u16, name: &str, kind: RecordType) -> Vec<u8> {
    let mut message = Message::new(id, MessageType::Query, OpCode::Query);
    message.metadata.recursion_desired = true;
    message.add_query(Query::query(Name::from_str(name).unwrap(), kind));
    message.to_vec().unwrap()
}

async fn ask_udp(addr: std::net::SocketAddr, packet: &[u8], wait: Duration) -> Option<Message> {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    socket.send_to(packet, addr).await.unwrap();
    let mut buf = vec![0u8; 4096];
    let (len, _) = tokio::time::timeout(wait, socket.recv_from(&mut buf))
        .await
        .ok()?
        .unwrap();
    Some(Message::from_vec(&buf[..len]).unwrap())
}

async fn add(storage: &dyn Storage, name: &str, kind: &str, value: &str) {
    let record = NewDnsRecord {
        name: name.into(),
        kind: kind.into(),
        value: value.into(),
        ttl: None,
    };
    storage.create_dns_record(record).await.unwrap();
}

#[tokio::test]
async fn answers_over_udp_and_tcp_and_truncates_what_does_not_fit() {
    let storage = mockhub_storage::connect("sqlite::memory:").await.unwrap();
    add(storage.as_ref(), "app.test", "A", "127.0.0.9").await;
    add(storage.as_ref(), "big.test", "TXT", &"x".repeat(900)).await;
    for (name, action) in [
        ("slow.test", FaultAction::Timeout),
        ("broken.test", FaultAction::Refused),
    ] {
        let fault = NewDnsFault {
            name: name.into(),
            kind: None,
            action,
            delay_ms: 0,
        };
        storage.create_dns_fault(fault).await.unwrap();
    }
    let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let udp_addr = udp.local_addr().unwrap();
    let tcp = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tcp_addr = tcp.local_addr().unwrap();
    tokio::spawn(serve_udp(udp, Engine::new(storage.clone())));
    tokio::spawn(serve_tcp(tcp, Engine::new(storage.clone())));
    let wait = Duration::from_secs(3);

    let got = ask_udp(udp_addr, &query(7, "APP.test.", RecordType::A), wait)
        .await
        .unwrap();
    assert_eq!(
        (got.metadata.id, got.metadata.response_code),
        (7, ResponseCode::NoError)
    );
    assert_eq!(got.answers[0].data.to_string(), "127.0.0.9");

    let got = ask_udp(udp_addr, &query(8, "nope.test.", RecordType::A), wait)
        .await
        .unwrap();
    assert_eq!(got.metadata.response_code, ResponseCode::NXDomain);
    let got = ask_udp(udp_addr, &query(9, "broken.test.", RecordType::A), wait)
        .await
        .unwrap();
    assert_eq!(got.metadata.response_code, ResponseCode::Refused);
    assert!(
        ask_udp(
            udp_addr,
            &query(10, "slow.test.", RecordType::A),
            Duration::from_millis(300)
        )
        .await
        .is_none()
    );
    assert!(
        ask_udp(udp_addr, b"not dns", Duration::from_millis(300))
            .await
            .is_none()
    );

    // 900 byte không vừa gói UDP 512 byte: TC, rồi TCP trả đủ.
    let packet = query(11, "big.test.", RecordType::TXT);
    let got = ask_udp(udp_addr, &packet, wait).await.unwrap();
    assert!(got.metadata.truncation);
    assert!(got.answers.is_empty());
    let mut stream = TcpStream::connect(tcp_addr).await.unwrap();
    stream.write_u16(packet.len() as u16).await.unwrap();
    stream.write_all(&packet).await.unwrap();
    let mut reply = vec![0u8; usize::from(stream.read_u16().await.unwrap())];
    stream.read_exact(&mut reply).await.unwrap();
    let got = Message::from_vec(&reply).unwrap();
    assert!(!got.metadata.truncation);
    assert_eq!(got.answers.len(), 1);
}

#[tokio::test]
async fn unowned_names_are_forwarded_and_a_cname_out_of_the_mock_is_chased_there() {
    // "Resolver thật" cũng là một mockhub khác.
    let real = mockhub_storage::connect("sqlite::memory:").await.unwrap();
    add(real.as_ref(), "api.other.test", "A", "9.9.9.9").await;
    add(real.as_ref(), "big.other.test", "TXT", &"y".repeat(900)).await;
    add(real.as_ref(), "owned.test", "A", "1.1.1.1").await;
    let udp = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = udp.local_addr().unwrap().to_string();
    let tcp = TcpListener::bind(&addr).await.unwrap();
    tokio::spawn(serve_udp(udp, Engine::new(real.clone())));
    tokio::spawn(serve_tcp(tcp, Engine::new(real)));

    let storage = mockhub_storage::connect("sqlite::memory:").await.unwrap();
    add(storage.as_ref(), "owned.test", "A", "127.0.0.9").await;
    add(storage.as_ref(), "www.shop.test", "CNAME", "api.other.test").await;
    let engine = Engine::new(storage.clone()).with_upstream(Some(&addr));
    let ask = async |name: &str, kind| {
        let Outcome::Reply(answer) = engine.lookup("udp", name, kind).await else {
            panic!("dropped");
        };
        let data: Vec<String> = answer.records.iter().map(|r| r.data.to_string()).collect();
        (answer.code, answer.source, data)
    };

    let owned = ask("owned.test", RecordType::A).await;
    assert_eq!((owned.1, owned.2), (Source::Mock, vec!["127.0.0.9".into()]));
    let nodata = ask("owned.test", RecordType::TXT).await;
    assert_eq!((nodata.0, nodata.1), (ResponseCode::NoError, Source::Mock));
    let forwarded = ask("api.other.test", RecordType::A).await;
    assert_eq!(forwarded.1, Source::Upstream);
    assert_eq!(forwarded.2, ["9.9.9.9"]);
    let chased = ask("www.shop.test", RecordType::A).await;
    assert_eq!(chased.2, ["api.other.test.", "9.9.9.9"]);
    assert_eq!(
        ask("none.other.test", RecordType::A).await.0,
        ResponseCode::NXDomain
    );
    // Quá một gói UDP: hỏi lại upstream qua TCP.
    assert_eq!(ask("big.other.test", RecordType::TXT).await.2.len(), 1);
    let log = storage.list_dns_queries(1).await.unwrap();
    assert_eq!(log[0].entry.source, "upstream");
}

#[tokio::test]
async fn an_upstream_that_does_not_answer_is_servfail() {
    let silent = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = silent.local_addr().unwrap().to_string();
    let storage = mockhub_storage::connect("sqlite::memory:").await.unwrap();
    let engine = Engine::new(storage).with_upstream(Some(&addr));
    let Outcome::Reply(answer) = engine.lookup("udp", "x.test", RecordType::A).await else {
        panic!("dropped");
    };
    assert_eq!(
        (answer.code, answer.source),
        (ResponseCode::ServFail, Source::Upstream)
    );
}

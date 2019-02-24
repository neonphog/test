extern crate tungstenite;
extern crate url;
extern crate native_tls;

mod connection_wss;

use crate::connection_wss::{ConnectionEvent, Connection};

fn main() {
    let mut con = connection_wss::ConnectionWss::with_std_tcp_stream();
    let cid = con.connect("wss://127.0.0.1:2794/").unwrap();
    con.send(vec![cid.clone()], "test-hello".as_bytes().to_vec()).unwrap();

    let mut last_send = std::time::Instant::now();
    let mut count = 1;
    loop {
        if last_send.elapsed() >= std::time::Duration::from_secs(1) {
            last_send = std::time::Instant::now();
            count += 1;
            con.send(
                vec![cid.clone()],
                format!("test {}", count).into_bytes()).unwrap();
        }
        let evt = con.poll().unwrap();
        match evt {
            ConnectionEvent::None => {
                std::thread::sleep(
                    std::time::Duration::from_millis(100));
            }
            ConnectionEvent::DidWork => {
                std::thread::sleep(
                    std::time::Duration::from_millis(1));
            }
            ConnectionEvent::Message(id, msg) => {
                println!("msg: {}", String::from_utf8_lossy(&msg));
            }
            _ => {
                println!("{:?}", evt);
            }
        }
    }

    /*
    let host = "127.0.0.1:2794";
    let url = format!("wss://{}/", host);

    let sock = std::net::TcpStream::connect(host).unwrap();
    sock.set_nonblocking(true).expect("set nonblocking failed");

    let connector = native_tls::TlsConnector::builder()
        .danger_accept_invalid_certs(true)
        .danger_accept_invalid_hostnames(true)
        .build()
        .expect("failed to build TlsConnector");

    let mut mid = connector.connect(&url, sock);

    let tls_sock = loop {
        match mid {
            Err(native_tls::HandshakeError::WouldBlock(m)) => {
                std::thread::sleep(std::time::Duration::from_millis(10));
                mid = m.handshake();
            },
            Err(e) => panic!("{:?}", e),
            Ok(m) => break m,
        }
    };

    let mut mid = tungstenite::client(
        url::Url::parse(&url).unwrap(), tls_sock);

    let (mut ws, response) = loop {
        match mid {
            Err(tungstenite::HandshakeError::Interrupted(m)) => {
                std::thread::sleep(std::time::Duration::from_millis(10));
                mid = m.handshake();
            },
            Err(e) => panic!("{:?}", e),
            Ok(m) => break m,
        }
    };

    ws.write_message(tungstenite::Message::Text("hallo".into())).unwrap();
    loop {
        let msg = match ws.read_message() {
            Err(tungstenite::error::Error::Io(e)) => {
                if e.kind() == std::io::ErrorKind::WouldBlock {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                    continue;
                }
                panic!("ee {:?} {:?}", e, e.kind());
            }
            Err(e) => panic!("{:?}", e),
            Ok(m) => m,
        };
        println!("got: {}", msg);
    }
    */
}

extern crate tungstenite;
extern crate url;
extern crate native_tls;

mod connection_error;
mod connection;
mod connection_wss;

use crate::connection::{ConnectionEvent, Connection};
use crate::connection_wss::ConnectionWss;

fn main() {
    let mut con = ConnectionWss::with_std_tcp_stream();
    con.connect("wss://127.0.0.1:2794/").unwrap();
    //con.send(vec![cid.clone()], "test-hello".as_bytes().to_vec()).unwrap();

    let mut cid: Option<String> = None;
    let mut last_send = std::time::Instant::now();
    let mut count = 1;
    loop {
        if last_send.elapsed() >= std::time::Duration::from_secs(1) {
            last_send = std::time::Instant::now();
            count += 1;
            if let Some(cid) = &cid {
                con.send(
                    vec![cid.clone()],
                    format!("test {}", count).into_bytes()).unwrap();
            }
        }
        let (did_work, evt_lst) = con.poll().unwrap();
        for evt in evt_lst {
            match evt {
                ConnectionEvent::ConnectionError(_id, e) => {
                    println!("got error: {:?}", e);
                    cid = None;
                }
                ConnectionEvent::Connect(id) => {
                    println!("connected!: {}", &id);
                    cid = Some(id);
                }
                ConnectionEvent::Close(id) => {
                    println!("got close: {}", &id);
                    return;
                }
                ConnectionEvent::Message(_id, msg) => {
                    println!("msg: {}", String::from_utf8_lossy(&msg));
                }
            }
        }
        if did_work {
            std::thread::sleep(
                std::time::Duration::from_millis(1));
        } else {
            std::thread::sleep(
                std::time::Duration::from_millis(100));
        }
    }
}

//! abstraction for working with connections

use std::io::{Read, Write};

use crate::connection_error::{ConnectionError, ConnectionResult};
use crate::connection::{ConnectionId, DidWork, ConnectionEvent, Connection};

type TlsConnectResult<T> = Result<TlsStream<T>, native_tls::HandshakeError<T>>;
type WssHandshakeError<T> = tungstenite::handshake::HandshakeError<tungstenite::handshake::client::ClientHandshake<TlsStream<T>>>;
type WssConnectResult<T> = Result<(WssStream<T>, tungstenite::handshake::client::Response), WssHandshakeError<T>>;

type BaseStream<T> = T;
type TlsMidHandshake<T> = native_tls::MidHandshakeTlsStream<BaseStream<T>>;
type TlsStream<T> = native_tls::TlsStream<BaseStream<T>>;
type WssMidHandshake<T> = tungstenite::handshake::MidHandshake<tungstenite::ClientHandshake<TlsStream<T>>>;
type WssStream<T> = tungstenite::protocol::WebSocket<TlsStream<T>>;

type StreamFactory<T> = fn(uri: &str) -> ConnectionResult<T>;

#[derive(Debug)]
enum WssStreamState<T: Read + Write + std::fmt::Debug> {
    None,
    Connecting(BaseStream<T>),
    TlsMidHandshake(TlsMidHandshake<T>),
    TlsReady(TlsStream<T>),
    WssMidHandshake(WssMidHandshake<T>),
    Ready(WssStream<T>),
}

#[derive(Debug)]
struct ConnectionInfo<T: Read + Write + std::fmt::Debug> {
    id: ConnectionId,
    url: url::Url,
    last_msg: std::time::Instant,
    socket: WssStreamState<T>,
}

type SocketMap<T> = std::collections::HashMap<String, ConnectionInfo<T>>;

pub struct ConnectionWss<T: Read + Write + std::fmt::Debug> {
    stream_factory: StreamFactory<T>,
    out_sockets: SocketMap<T>,
    event_queue: Vec<ConnectionEvent>,
    n_id: u64,
}

impl<T: Read + Write + std::fmt::Debug> ConnectionWss<T> {
    pub fn new(stream_factory: StreamFactory<T>) -> Self {
        ConnectionWss {
            stream_factory,
            out_sockets: std::collections::HashMap::new(),
            event_queue: Vec::new(),
            n_id: 1,
        }
    }

    // -- private -- //

    fn priv_next_id(&mut self) -> String {
        let out = format!("ws{}", self.n_id);
        self.n_id += 1;
        return out;
    }

    fn priv_process_out_sockets(&mut self) -> ConnectionResult<bool> {
        let mut did_work = false;

        // take sockets out, so we can mut ref into self and it at same time
        let sockets: Vec<(String, ConnectionInfo<T>)> =
            self.out_sockets.drain().collect();

        for (id, mut info) in sockets {
            if let Err(e) = self.priv_process_socket(&mut did_work, &mut info) {
                self.event_queue.push(ConnectionEvent::ConnectionError(info.id.clone(), e));
            }
            if let WssStreamState::None = info.socket {
                self.event_queue.push(ConnectionEvent::Close(info.id));
                continue;
            }
            if info.last_msg.elapsed().as_millis() > 200 {
                if let WssStreamState::Ready(socket) = &mut info.socket {
                    socket.write_message(tungstenite::Message::Ping(vec![]))?;
                }
            } else if info.last_msg.elapsed().as_millis() > 500 {
                self.event_queue.push(ConnectionEvent::Close(info.id));
                info.socket = WssStreamState::None;
                continue;
            }
            self.out_sockets.insert(id, info);
        }

        Ok(did_work)
    }

    fn priv_process_socket(&mut self, did_work: &mut bool, info: &mut ConnectionInfo<T>) -> ConnectionResult<()> {
        // move the socket out, to be replaced
        let socket = std::mem::replace(&mut info.socket, WssStreamState::None);

        match socket {
            WssStreamState::None => {
                // stream must have closed, do nothing
                return Ok(());
            }
            WssStreamState::Connecting(socket) => {
                info.last_msg = std::time::Instant::now();
                *did_work = true;
                let connector = native_tls::TlsConnector::builder()
                    .danger_accept_invalid_certs(true)
                    .danger_accept_invalid_hostnames(true)
                    .build()
                    .expect("failed to build TlsConnector");
                info.socket = self.priv_tls_handshake(connector.connect(info.url.as_str(), socket))?;
                return Ok(());
            }
            WssStreamState::TlsMidHandshake(socket) => {
                info.socket = self.priv_tls_handshake(socket.handshake())?;
                return Ok(());
            }
            WssStreamState::TlsReady(socket) => {
                info.last_msg = std::time::Instant::now();
                *did_work = true;
                info.socket = self.priv_ws_handshake(&info.id, tungstenite::client(info.url.clone(), socket))?;
                return Ok(());
            }
            WssStreamState::WssMidHandshake(socket) => {
                info.socket = self.priv_ws_handshake(&info.id, socket.handshake())?;
                return Ok(());
            }
            WssStreamState::Ready(mut socket) => {
                match socket.read_message() {
                    Err(tungstenite::error::Error::Io(e)) => {
                        if e.kind() == std::io::ErrorKind::WouldBlock {
                            info.socket = WssStreamState::Ready(socket);
                            return Ok(());
                        }
                        return Err(e.into());
                    }
                    Err(tungstenite::error::Error::ConnectionClosed(_)) => {
                        // close event will be published
                        return Ok(());
                    }
                    Err(e) => {
                        return Err(e.into());
                    }
                    Ok(msg) => {
                        info.last_msg = std::time::Instant::now();
                        *did_work = true;
                        let mut qmsg = None;
                        match msg {
                            tungstenite::Message::Text(s) => qmsg = Some(s.into_bytes()),
                            tungstenite::Message::Binary(b) => qmsg = Some(b),
                            _ => ()
                        }
                        if let Some(msg) = qmsg {
                            self.event_queue.push(
                                ConnectionEvent::Message(info.id.clone(), msg));
                        }
                        info.socket = WssStreamState::Ready(socket);
                        return Ok(());
                    }
                }
            }
        }
    }

    fn priv_tls_handshake(&mut self, res: TlsConnectResult<T>) -> ConnectionResult<WssStreamState<T>> {
        match res {
            Err(native_tls::HandshakeError::WouldBlock(socket)) => {
                Ok(WssStreamState::TlsMidHandshake(socket))
            }
            Err(e) => {
                Err(e.into())
            }
            Ok(socket) => {
                Ok(WssStreamState::TlsReady(socket))
            }
        }
    }

    fn priv_ws_handshake(&mut self, id: &ConnectionId, res: WssConnectResult<T>) -> ConnectionResult<WssStreamState<T>> {
        match res {
            Err(tungstenite::HandshakeError::Interrupted(socket)) => {
                Ok(WssStreamState::WssMidHandshake(socket))
            }
            Err(e) => {
                Err(e.into())
            }
            Ok((socket, _response)) => {
                self.event_queue.push(ConnectionEvent::Connect(id.clone()));
                Ok(WssStreamState::Ready(socket))
            }
        }
    }
}

impl ConnectionWss<std::net::TcpStream> {
    pub fn with_std_tcp_stream() -> Self {
        ConnectionWss::new(|uri| {
            let socket = std::net::TcpStream::connect(uri)?;
            socket.set_nonblocking(true)?;
            Ok(socket)
        })
    }
}

impl<T: Read + Write + std::fmt::Debug> Connection for ConnectionWss<T> {
    fn connect(&mut self, uri: &str) -> ConnectionResult<ConnectionId> {
        let uri = url::Url::parse(uri)?;
        let host_port = format!("{}:{}",
            uri.host_str().ok_or(ConnectionError("bad connect host".into()))?,
            uri.port().ok_or(ConnectionError("bad connect port".into()))?,
        );
        let socket = (self.stream_factory)(&host_port)?;
        let id = self.priv_next_id();
        let info = ConnectionInfo {
            id: id.clone(),
            url: uri,
            last_msg: std::time::Instant::now(),
            socket: WssStreamState::Connecting(socket),
        };
        self.out_sockets.insert(id.clone(), info);
        Ok(id)
    }

    fn close(&mut self, id: ConnectionId) -> ConnectionResult<()> {
        if let Some(info) = self.out_sockets.get_mut(&id) {
            if let WssStreamState::Ready(socket) = &mut info.socket {
                socket.close(None)?;
            }
            info.socket = WssStreamState::None;
        } else {
            return Err(ConnectionError(format!("bad id: {}", id)));
        }
        Ok(())
    }

    fn poll(&mut self) -> ConnectionResult<(DidWork, Vec<ConnectionEvent>)> {
        let mut did_work = false;

        if self.priv_process_out_sockets()? {
            did_work = true
        }

        Ok((did_work, self.event_queue.drain(..).collect()))
    }

    fn send(&mut self, id_list: Vec<ConnectionId>, payload: Vec<u8>) -> ConnectionResult<()> {
        for id in id_list {
            if let Some(info) = self.out_sockets.get_mut(&id) {
                if let WssStreamState::Ready(socket) = &mut info.socket {
                    socket.write_message(
                        tungstenite::Message::Binary(payload.clone()))?;
                } else {
                    return Err(ConnectionError(format!("bad stream state: {:?}", info.socket)));
                }
            } else {
                return Err(ConnectionError(format!("bad id: {}", id)));
            }
        }
        Ok(())
    }
}

//! abstraction for working with connections

use std::io::{Read, Write};

#[derive(Debug, PartialEq, Clone)]
pub struct ConnectionError(pub String);

impl std::fmt::Display for ConnectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl std::error::Error for ConnectionError {
    fn description(&self) -> &str {
        &self.0
    }
    fn cause(&self) -> Option<&std::error::Error> {
        None
    }
}

impl From<url::ParseError> for ConnectionError {
    fn from(error: url::ParseError) -> Self {
        Self(format!("{:?}", error))
    }
}

impl From<std::io::Error> for ConnectionError {
    fn from(error: std::io::Error) -> Self {
        Self(format!("{:?}", error))
    }
}

impl From<tungstenite::Error> for ConnectionError {
    fn from(error: tungstenite::Error) -> Self {
        Self(format!("{:?}", error))
    }
}

impl<T: Read + Write + std::fmt::Debug> From<native_tls::HandshakeError<T>> for ConnectionError {
    fn from(error: native_tls::HandshakeError<T>) -> Self {
        Self(format!("{:?}", error))
    }
}

impl<T: Read + Write + std::fmt::Debug> From<tungstenite::HandshakeError<tungstenite::ClientHandshake<T>>> for ConnectionError {
    fn from(error: tungstenite::HandshakeError<tungstenite::ClientHandshake<T>>) -> Self {
        Self(format!("{:?}", error))
    }
}

type TlsConnectResult<T> = Result<TlsStream<T>, native_tls::HandshakeError<T>>;
type WssHandshakeError<T> = tungstenite::handshake::HandshakeError<tungstenite::handshake::client::ClientHandshake<TlsStream<T>>>;
type WssConnectResult<T> = Result<(WssStream<T>, tungstenite::handshake::client::Response), WssHandshakeError<T>>;

pub type ConnectionResult<T> = Result<T, ConnectionError>;

pub type ConnectionId = String;

#[derive(Debug, PartialEq, Clone)]
pub enum ConnectionEvent {
    None,
    DidWork,
    ConnectionError(ConnectionId, ConnectionError),
    Connect(ConnectionId),
    Message(ConnectionId, Vec<u8>),
    Close(ConnectionId),
}

pub trait Connection {
    fn connect(&mut self, uri: &str) -> ConnectionResult<ConnectionId>;
    fn poll(&mut self) -> ConnectionResult<ConnectionEvent>;
    fn send(&mut self, id_list: Vec<ConnectionId>, payload: Vec<u8>) -> ConnectionResult<()>;
}

type TcpStream<T> = T;
type TlsMidHandshake<T> = native_tls::MidHandshakeTlsStream<TcpStream<T>>;
type TlsStream<T> = native_tls::TlsStream<TcpStream<T>>;
type WssMidHandshake<T> = tungstenite::handshake::MidHandshake<tungstenite::ClientHandshake<TlsStream<T>>>;
type WssStream<T> = tungstenite::protocol::WebSocket<TlsStream<T>>;

type StreamFactory<T> = fn(uri: &str) -> ConnectionResult<T>;

#[derive(Debug)]
enum WssStreamState<T: Read + Write + std::fmt::Debug> {
    Connecting(TcpStream<T>),
    TlsMidHandshake(TlsMidHandshake<T>),
    TlsReady(TlsStream<T>),
    WssMidHandshake(WssMidHandshake<T>),
    Ready(WssStream<T>),
}

#[derive(Debug, Clone)]
struct ConnectionInfo {
    id: ConnectionId,
    url: url::Url,
}

type SocketList<T> = Vec<(ConnectionInfo, WssStreamState<T>)>;

pub struct ConnectionWss<T: Read + Write + std::fmt::Debug> {
    stream_factory: StreamFactory<T>,
    out_sockets: SocketList<T>,
    event_queue: Vec<ConnectionEvent>,
    send_queue: Vec<(Vec<ConnectionId>, Vec<u8>)>,
}

impl<T: Read + Write + std::fmt::Debug> ConnectionWss<T> {
    pub fn new(stream_factory: StreamFactory<T>) -> Self {
        ConnectionWss {
            stream_factory,
            out_sockets: Vec::new(),
            event_queue: Vec::new(),
            send_queue: Vec::new(),
        }
    }

    // -- private -- //

    fn priv_process_out_sockets(&mut self) -> ConnectionResult<bool> {
        let mut did_work = false;

        let mut new_out_sockets = Vec::new();
        let loop_sockets: SocketList<T> = self.out_sockets.drain(..).collect();
        for (info, socket) in loop_sockets {
            match self.priv_process_socket(&mut did_work, info.clone(), socket) {
                Err(e) => self.event_queue.push(ConnectionEvent::ConnectionError(info.id, e)),
                Ok(s) => {
                    new_out_sockets.push(s);
                }
            }
        }
        self.out_sockets = new_out_sockets;

        Ok(did_work)
    }

    fn priv_process_socket(&mut self, did_work: &mut bool, info: ConnectionInfo, socket: WssStreamState<T>) -> ConnectionResult<(ConnectionInfo, WssStreamState<T>)> {
        match socket {
            WssStreamState::Connecting(socket) => {
                *did_work = true;
                let connector = native_tls::TlsConnector::builder()
                    .danger_accept_invalid_certs(true)
                    .danger_accept_invalid_hostnames(true)
                    .build()
                    .expect("failed to build TlsConnector");
                return Ok((info.clone(), self.priv_tls_handshake(connector.connect(info.url.as_str(), socket))?));
            }
            WssStreamState::TlsMidHandshake(socket) => {
                *did_work = true;
                return Ok((info, self.priv_tls_handshake(socket.handshake())?));
            }
            WssStreamState::TlsReady(socket) => {
                *did_work = true;
                let res = tungstenite::client(info.url.clone(), socket);
                let res = self.priv_ws_handshake(res)?;
                return Ok((info, res));
            }
            WssStreamState::WssMidHandshake(socket) => {
                *did_work = true;
                return Ok((info, self.priv_ws_handshake(socket.handshake())?));
            }
            WssStreamState::Ready(mut socket) => {
                // XXX beg - HACK
                if !self.send_queue.is_empty() {
                    let (_, msg) = self.send_queue.remove(0);
                    socket.write_message(tungstenite::Message::Binary(msg));
                }
                // XXX end - HACK
                match socket.read_message() {
                    Err(tungstenite::error::Error::Io(e)) => {
                        if e.kind() == std::io::ErrorKind::WouldBlock {
                            return Ok((info, WssStreamState::Ready(socket)));
                        }
                        return Err(e.into());
                    }
                    Err(e) => {
                        return Err(e.into())
                    }
                    Ok(msg) => {
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
                        return Ok((info, WssStreamState::Ready(socket)));
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

    fn priv_ws_handshake(&mut self, res: WssConnectResult<T>) -> ConnectionResult<WssStreamState<T>> {
        match res {
            Err(tungstenite::HandshakeError::Interrupted(socket)) => {
                Ok(WssStreamState::WssMidHandshake(socket))
            }
            Err(e) => {
                Err(e.into())
            }
            Ok((socket, _response)) => {
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
        let info = ConnectionInfo {
            id: "fake".to_string(),
            url: uri,
        };
        self.out_sockets.push((info.clone(), WssStreamState::Connecting(socket)));
        Ok(info.id)
    }

    fn poll(&mut self) -> ConnectionResult<ConnectionEvent> {
        if !self.event_queue.is_empty() {
            return Ok(self.event_queue.remove(0))
        }

        let mut did_work = false;

        if self.priv_process_out_sockets()? {
            did_work = true
        }

        if !self.event_queue.is_empty() {
            return Ok(self.event_queue.remove(0));
        } else if did_work {
            return Ok(ConnectionEvent::DidWork);
        }
        Ok(ConnectionEvent::None)
    }

    fn send(&mut self, id_list: Vec<ConnectionId>, payload: Vec<u8>) -> ConnectionResult<()> {
        self.send_queue.push((id_list, payload));
        Ok(())
    }
}

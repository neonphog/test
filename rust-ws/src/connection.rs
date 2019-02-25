pub type ConnectionId = String;

use crate::connection_error::{ConnectionError, ConnectionResult};

pub type DidWork = bool;

#[derive(Debug, PartialEq, Clone)]
pub enum ConnectionEvent {
    ConnectionError(ConnectionId, ConnectionError),
    Connect(ConnectionId),
    Message(ConnectionId, Vec<u8>),
    Close(ConnectionId),
}

pub trait Connection {
    fn connect(&mut self, uri: &str) -> ConnectionResult<ConnectionId>;
    fn close(&mut self, id: ConnectionId) -> ConnectionResult<()>;
    fn poll(&mut self) -> ConnectionResult<(DidWork, Vec<ConnectionEvent>)>;
    fn send(&mut self, id_list: Vec<ConnectionId>, payload: Vec<u8>) -> ConnectionResult<()>;
}

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

impl<T: std::io::Read + std::io::Write + std::fmt::Debug> From<native_tls::HandshakeError<T>> for ConnectionError {
    fn from(error: native_tls::HandshakeError<T>) -> Self {
        Self(format!("{:?}", error))
    }
}

impl<T: std::io::Read + std::io::Write + std::fmt::Debug> From<tungstenite::HandshakeError<tungstenite::ClientHandshake<T>>> for ConnectionError {
    fn from(error: tungstenite::HandshakeError<tungstenite::ClientHandshake<T>>) -> Self {
        Self(format!("{:?}", error))
    }
}

pub type ConnectionResult<T> = Result<T, ConnectionError>;

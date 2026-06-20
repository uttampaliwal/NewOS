use std::collections::HashMap;

use ipc_proto::{encode_message, decode_message, IpcError, IpcMessage, IpcValue};

// ---------------------------------------------------------------------------
// BrokerError
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum BrokerError {
    TransportError(String),
    ProtocolError(String),
    ServiceNotFound(String),
    MethodNotFound(String),
    ConnectionClosed(u64),
}

impl std::fmt::Display for BrokerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BrokerError::TransportError(msg) => write!(f, "transport error: {msg}"),
            BrokerError::ProtocolError(msg) => write!(f, "protocol error: {msg}"),
            BrokerError::ServiceNotFound(name) => write!(f, "service not found: {name}"),
            BrokerError::MethodNotFound(name) => write!(f, "method not found: {name}"),
            BrokerError::ConnectionClosed(id) => write!(f, "connection {id} closed"),
        }
    }
}

impl std::error::Error for BrokerError {}

// ---------------------------------------------------------------------------
// Transport trait
// ---------------------------------------------------------------------------

/// Abstraction over a bidirectional message stream.
pub trait Transport: Send {
    /// Send an encoded message (the transport writes length-prefixed bytes).
    fn send(&mut self, msg: &IpcMessage) -> Result<(), BrokerError>;
    /// Receive an encoded message (blocking).
    fn recv(&mut self) -> Result<IpcMessage, BrokerError>;
    /// Close the transport.
    fn close(&mut self) -> Result<(), BrokerError>;
}

// ---------------------------------------------------------------------------
// ChannelTransport — in-memory channel pair for testing
// ---------------------------------------------------------------------------

pub struct ChannelTransport {
    tx: smol::channel::Sender<IpcMessage>,
    rx: smol::channel::Receiver<IpcMessage>,
}

impl ChannelTransport {
    pub fn new_pair() -> (ChannelTransport, ChannelTransport) {
        let (tx_a, rx_a) = smol::channel::unbounded();
        let (tx_b, rx_b) = smol::channel::unbounded();
        let a = ChannelTransport { tx: tx_a, rx: rx_b };
        let b = ChannelTransport { tx: tx_b, rx: rx_a };
        (a, b)
    }
}

impl Transport for ChannelTransport {
    fn send(&mut self, msg: &IpcMessage) -> Result<(), BrokerError> {
        self.tx
            .try_send(msg.clone())
            .map_err(|e| BrokerError::TransportError(e.to_string()))
    }

    fn recv(&mut self) -> Result<IpcMessage, BrokerError> {
        smol::block_on(self.rx.recv())
            .map_err(|e| BrokerError::TransportError(e.to_string()))
    }

    fn close(&mut self) -> Result<(), BrokerError> {
        self.tx.close();
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// SocketTransport — Unix domain socket transport
// ---------------------------------------------------------------------------

pub struct SocketTransport {
    stream: std::io::BufReader<std::os::unix::net::UnixStream>,
    write_stream: std::os::unix::net::UnixStream,
}

impl SocketTransport {
    pub fn connect(path: &str) -> Result<Self, BrokerError> {
        let stream = std::os::unix::net::UnixStream::connect(path)
            .map_err(|e| BrokerError::TransportError(format!("connect failed: {e}")))?;
        let write_stream = stream.try_clone()
            .map_err(|e| BrokerError::TransportError(format!("clone failed: {e}")))?;
        Ok(Self {
            stream: std::io::BufReader::new(stream),
            write_stream,
        })
    }

    pub fn from_stream(stream: std::os::unix::net::UnixStream) -> Self {
        let write_stream = stream.try_clone().unwrap();
        Self {
            stream: std::io::BufReader::new(stream),
            write_stream,
        }
    }
}

impl Transport for SocketTransport {
    fn send(&mut self, msg: &IpcMessage) -> Result<(), BrokerError> {
        use std::io::Write;
        let bytes = encode_message(msg)
            .map_err(BrokerError::TransportError)?;
        self.write_stream
            .write_all(&bytes)
            .map_err(|e| BrokerError::TransportError(format!("write failed: {e}")))?;
        self.write_stream
            .flush()
            .map_err(|e| BrokerError::TransportError(format!("flush failed: {e}")))?;
        Ok(())
    }

    fn recv(&mut self) -> Result<IpcMessage, BrokerError> {
        use std::io::Read;
        // Read length prefix (4 bytes)
        let mut len_buf = [0u8; 4];
        self.stream
            .read_exact(&mut len_buf)
            .map_err(|e| BrokerError::TransportError(format!("read length failed: {e}")))?;
        let len = u32::from_le_bytes(len_buf) as usize;

        // Read payload
        let mut payload = vec![0u8; len];
        self.stream
            .read_exact(&mut payload)
            .map_err(|e| BrokerError::TransportError(format!("read payload failed: {e}")))?;

        let mut full = Vec::with_capacity(4 + len);
        full.extend_from_slice(&len_buf);
        full.extend_from_slice(&payload);

        let (msg, _) = decode_message(&full)
            .map_err(BrokerError::ProtocolError)?;
        Ok(msg)
    }

    fn close(&mut self) -> Result<(), BrokerError> {
        use std::net::Shutdown;
        self.write_stream
            .shutdown(Shutdown::Both)
            .map_err(|e| BrokerError::TransportError(format!("shutdown failed: {e}")))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Connection
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServiceRegistration {
    pub interface: String,
    pub methods: Vec<String>,
}

#[allow(dead_code)]
struct ConnectionState {
    id: u64,
    registration: Option<ServiceRegistration>,
    // For in-memory channel transport; None for socket transport
    tx: Option<smol::channel::Sender<IpcMessage>>,
}

// ---------------------------------------------------------------------------
// Broker
// ---------------------------------------------------------------------------

/// The IPC broker manages service registration, method call routing, and
/// signal broadcast.
pub struct Broker {
    next_conn_id: u64,
    /// Registered services: interface → connection_id
    registry: HashMap<String, u64>,
    /// Connection details
    connections: HashMap<u64, ConnectionState>,
    /// Subscribers for signal interfaces
    subscribers: HashMap<String, Vec<u64>>,
    /// Pending method calls: call_id → caller_connection_id
    pending_calls: HashMap<u64, u64>,
}

impl Broker {
    pub fn new() -> Self {
        Self {
            next_conn_id: 1,
            registry: HashMap::new(),
            connections: HashMap::new(),
            subscribers: HashMap::new(),
            pending_calls: HashMap::new(),
        }
    }

    /// Accept a new connection via the given transport.
    /// Returns the connection ID.
    pub fn accept(
        &mut self,
        tx: Option<smol::channel::Sender<IpcMessage>>,
    ) -> u64 {
        let id = self.next_conn_id;
        self.next_conn_id += 1;
        self.connections.insert(
            id,
            ConnectionState {
                id,
                registration: None,
                tx,
            },
        );
        id
    }

    /// Register a service on the given connection.
    pub fn register_service(
        &mut self,
        conn_id: u64,
        registration: ServiceRegistration,
    ) -> Result<(), BrokerError> {
        let conn = self.connections.get_mut(&conn_id)
            .ok_or_else(|| BrokerError::ConnectionClosed(conn_id))?;
        conn.registration = Some(registration.clone());
        self.registry.insert(registration.interface.clone(), conn_id);
        Ok(())
    }

    /// Handle an incoming message from a connection.
    /// Returns an optional reply to send back to the caller.
    pub fn handle_message(
        &mut self,
        conn_id: u64,
        msg: IpcMessage,
    ) -> Result<Vec<(u64, IpcMessage)>, BrokerError> {
        match msg {
            IpcMessage::MethodCall { id, interface, method, args } => {
                self.route_method_call(conn_id, id, &interface, &method, args)
            }
            IpcMessage::MethodReturn { id, result } => {
                self.route_method_return(conn_id, id, result)
            }
            IpcMessage::Signal { interface, name, args } => {
                self.broadcast_signal(conn_id, interface, name, args)
            }
            IpcMessage::PropertyGet { id, interface, name } => {
                self.route_property_get(conn_id, id, &interface, &name)
            }
            IpcMessage::PropertySet { id, interface, name, value } => {
                self.route_property_set(conn_id, id, &interface, &name, value)
            }
        }
    }

    /// Disconnect a connection.
    pub fn disconnect(&mut self, conn_id: u64) {
        // Remove from registry
        if let Some(conn) = self.connections.get(&conn_id) {
            if let Some(ref reg) = conn.registration {
                self.registry.remove(&reg.interface);
                self.subscribers.remove(&reg.interface);
            }
        }
        self.connections.remove(&conn_id);
        // Clean up pending calls from this connection
        self.pending_calls.retain(|_, v| *v != conn_id);
    }

    // -----------------------------------------------------------------------
    // Internal routing
    // -----------------------------------------------------------------------

    fn route_method_call(
        &mut self,
        caller_id: u64,
        call_id: u64,
        interface: &str,
        method: &str,
        args: Vec<IpcValue>,
    ) -> Result<Vec<(u64, IpcMessage)>, BrokerError> {
        let target_id = self.registry.get(interface)
            .ok_or_else(|| BrokerError::ServiceNotFound(interface.to_string()))?;

        // Check the service has that method
        if let Some(conn) = self.connections.get(target_id) {
            if let Some(ref reg) = conn.registration {
                if !reg.methods.is_empty() && !reg.methods.contains(&method.to_string()) {
                    return Err(BrokerError::MethodNotFound(method.to_string()));
                }
            }
        }

        self.pending_calls.insert(call_id, caller_id);
        let forward = IpcMessage::MethodCall {
            id: call_id,
            interface: interface.to_string(),
            method: method.to_string(),
            args,
        };
        Ok(vec![(*target_id, forward)])
    }

    fn route_method_return(
        &mut self,
        _responder_id: u64,
        call_id: u64,
        result: Result<IpcValue, IpcError>,
    ) -> Result<Vec<(u64, IpcMessage)>, BrokerError> {
        let caller_id = self.pending_calls.remove(&call_id)
            .ok_or_else(|| BrokerError::ProtocolError(format!("unknown call id {call_id}")))?;
        let reply = IpcMessage::MethodReturn {
            id: call_id,
            result,
        };
        Ok(vec![(caller_id, reply)])
    }

    fn broadcast_signal(
        &mut self,
        _sender_id: u64,
        interface: String,
        name: String,
        args: Vec<IpcValue>,
    ) -> Result<Vec<(u64, IpcMessage)>, BrokerError> {
        let signal = IpcMessage::Signal {
            interface: interface.clone(),
            name: name.clone(),
            args,
        };
        let subscribers = self.subscribers.entry(interface).or_default();
        let targets: Vec<u64> = subscribers.clone();
        Ok(targets.into_iter().map(|id| (id, signal.clone())).collect())
    }

    fn route_property_get(
        &mut self,
        caller_id: u64,
        call_id: u64,
        interface: &str,
        _name: &str,
    ) -> Result<Vec<(u64, IpcMessage)>, BrokerError> {
        let target_id = self.registry.get(interface)
            .ok_or_else(|| BrokerError::ServiceNotFound(interface.to_string()))?;
        self.pending_calls.insert(call_id, caller_id);
        let forward = IpcMessage::PropertyGet {
            id: call_id,
            interface: interface.to_string(),
            name: _name.to_string(),
        };
        Ok(vec![(*target_id, forward)])
    }

    fn route_property_set(
        &mut self,
        caller_id: u64,
        call_id: u64,
        interface: &str,
        _name: &str,
        value: IpcValue,
    ) -> Result<Vec<(u64, IpcMessage)>, BrokerError> {
        let target_id = self.registry.get(interface)
            .ok_or_else(|| BrokerError::ServiceNotFound(interface.to_string()))?;
        self.pending_calls.insert(call_id, caller_id);
        let forward = IpcMessage::PropertySet {
            id: call_id,
            interface: interface.to_string(),
            name: _name.to_string(),
            value,
        };
        Ok(vec![(*target_id, forward)])
    }
}

impl Default for Broker {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_service_registration(interface: &str, methods: &[&str]) -> ServiceRegistration {
        ServiceRegistration {
            interface: interface.to_string(),
            methods: methods.iter().map(|m| m.to_string()).collect(),
        }
    }

    #[test]
    fn test_service_registration_and_method_call() {
        let mut broker = Broker::new();

        // Service connects
        let svc_id = broker.accept(None);
        broker
            .register_service(svc_id, make_service_registration("com.test.Echo", &["ping"]))
            .unwrap();

        // Client connects and sends a method call
        let client_id = broker.accept(None);
        let msg = IpcMessage::MethodCall {
            id: 1,
            interface: "com.test.Echo".into(),
            method: "ping".into(),
            args: vec![],
        };
        let outputs = broker.handle_message(client_id, msg).unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].0, svc_id);

        // Service responds
        let reply = IpcMessage::MethodReturn {
            id: 1,
            result: Ok(IpcValue::String("pong".into())),
        };
        let outputs = broker.handle_message(svc_id, reply).unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].0, client_id);
        match &outputs[0].1 {
            IpcMessage::MethodReturn { id, result } => {
                assert_eq!(*id, 1);
                assert!(result.is_ok());
                assert_eq!(result.as_ref().unwrap().as_str(), Some("pong"));
            }
            _ => panic!("expected MethodReturn"),
        }
    }

    #[test]
    fn test_method_call_unregistered_service() {
        let mut broker = Broker::new();
        let client_id = broker.accept(None);

        let msg = IpcMessage::MethodCall {
            id: 1,
            interface: "com.test.Nonexistent".into(),
            method: "foo".into(),
            args: vec![],
        };
        let result = broker.handle_message(client_id, msg);
        assert!(matches!(result, Err(BrokerError::ServiceNotFound(_))));
    }

    #[test]
    fn test_method_call_unknown_method() {
        let mut broker = Broker::new();
        let svc_id = broker.accept(None);
        broker
            .register_service(svc_id, make_service_registration("com.test.Foo", &["bar"]))
            .unwrap();

        let client_id = broker.accept(None);
        let msg = IpcMessage::MethodCall {
            id: 1,
            interface: "com.test.Foo".into(),
            method: "nonexistent".into(),
            args: vec![],
        };
        let result = broker.handle_message(client_id, msg);
        assert!(matches!(result, Err(BrokerError::MethodNotFound(_))));
    }

    #[test]
    fn test_method_return_unknown_call_id() {
        let mut broker = Broker::new();
        let svc_id = broker.accept(None);

        let reply = IpcMessage::MethodReturn {
            id: 999,
            result: Ok(IpcValue::Null),
        };
        let result = broker.handle_message(svc_id, reply);
        assert!(matches!(result, Err(BrokerError::ProtocolError(_))));
    }

    #[test]
    fn test_signal_broadcast() {
        let mut broker = Broker::new();
        let svc_id = broker.accept(None);
        broker
            .register_service(svc_id, make_service_registration("com.test.Events", &[]))
            .unwrap();

        // Subscribe a second connection to the interface
        let sub_id = broker.accept(None);
        broker.subscribers.entry("com.test.Events".into()).or_default().push(sub_id);

        // Send a signal from the service
        let signal = IpcMessage::Signal {
            interface: "com.test.Events".into(),
            name: "UserJoined".into(),
            args: vec![IpcValue::String("alice".into())],
        };
        let outputs = broker.handle_message(svc_id, signal).unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].0, sub_id);
        match &outputs[0].1 {
            IpcMessage::Signal { name, args, .. } => {
                assert_eq!(name, "UserJoined");
                assert_eq!(args[0].as_str(), Some("alice"));
            }
            _ => panic!("expected Signal"),
        }
    }

    #[test]
    fn test_disconnect_cleans_up_registry() {
        let mut broker = Broker::new();
        let svc_id = broker.accept(None);
        broker
            .register_service(svc_id, make_service_registration("com.test.Svc", &["do"]))
            .unwrap();

        assert!(broker.registry.contains_key("com.test.Svc"));

        broker.disconnect(svc_id);
        assert!(!broker.registry.contains_key("com.test.Svc"));
    }

    #[test]
    fn test_property_get_forwarding() {
        let mut broker = Broker::new();
        let svc_id = broker.accept(None);
        broker
            .register_service(svc_id, make_service_registration("com.test.Config", &[]))
            .unwrap();

        let client_id = broker.accept(None);
        let msg = IpcMessage::PropertyGet {
            id: 1,
            interface: "com.test.Config".into(),
            name: "Theme".into(),
        };
        let outputs = broker.handle_message(client_id, msg).unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].0, svc_id);
        match &outputs[0].1 {
            IpcMessage::PropertyGet { name, .. } => {
                assert_eq!(name, "Theme");
            }
            _ => panic!("expected PropertyGet"),
        }
    }

    #[test]
    fn test_property_set_forwarding() {
        let mut broker = Broker::new();
        let svc_id = broker.accept(None);
        broker
            .register_service(svc_id, make_service_registration("com.test.Config", &[]))
            .unwrap();

        let client_id = broker.accept(None);
        let msg = IpcMessage::PropertySet {
            id: 1,
            interface: "com.test.Config".into(),
            name: "Volume".into(),
            value: IpcValue::Int(75),
        };
        let outputs = broker.handle_message(client_id, msg).unwrap();
        assert_eq!(outputs.len(), 1);
        assert_eq!(outputs[0].0, svc_id);
    }

    #[test]
    fn test_channel_transport_round_trip() {
        let (mut client, mut server) = ChannelTransport::new_pair();

        let msg = IpcMessage::MethodCall {
            id: 1,
            interface: "com.test".into(),
            method: "hello".into(),
            args: vec![IpcValue::String("world".into())],
        };

        client.send(&msg).unwrap();
        let received = server.recv().unwrap();
        assert_eq!(msg, received);
    }

    #[test]
    fn test_multiple_registrations() {
        let mut broker = Broker::new();

        let svc1 = broker.accept(None);
        broker
            .register_service(svc1, make_service_registration("com.alpha", &["a"]))
            .unwrap();

        let svc2 = broker.accept(None);
        broker
            .register_service(svc2, make_service_registration("com.beta", &["b"]))
            .unwrap();

        // Both should be reachable
        let client = broker.accept(None);
        let msg_a = IpcMessage::MethodCall {
            id: 1,
            interface: "com.alpha".into(),
            method: "a".into(),
            args: vec![],
        };
        let outputs = broker.handle_message(client, msg_a).unwrap();
        assert_eq!(outputs[0].0, svc1);

        let msg_b = IpcMessage::MethodCall {
            id: 2,
            interface: "com.beta".into(),
            method: "b".into(),
            args: vec![],
        };
        let outputs = broker.handle_message(client, msg_b).unwrap();
        assert_eq!(outputs[0].0, svc2);
    }
}

use ipc_broker::{Broker, BrokerError, ServiceRegistration, SocketTransport, Transport};
use std::os::unix::net::UnixListener;

fn main() {
    // Remove stale socket file
    let socket_path = "/run/ipc.sock";
    let _ = std::fs::remove_file(socket_path);

    let listener = match UnixListener::bind(socket_path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("ipc-broker: failed to bind {socket_path}: {e}");
            std::process::exit(1);
        }
    };

    let mut broker = Broker::new(false);
    let mut next_conn_id = 1u64;

    // Accept connections sequentially (simple approach; in production a
    // thread pool or async reactor would be used).
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let mut transport = SocketTransport::from_stream(stream);
                let conn_id = next_conn_id;
                next_conn_id += 1;

                // Read registration from the connecting service.
                match transport.recv() {
                    Ok(msg) => {
                        // The first message from a service must be a MethodCall
                        // to "org.turnix.Broker" / "Register" conveying the
                        // interface name and methods.
                        match &msg {
                            turnix_ipc_proto::IpcMessage::MethodCall {
                                id: _,
                                interface,
                                method,
                                args,
                            } if interface == "org.turnix.Broker"
                              && method == "Register" =>
                            {
                                let iface = args
                                    .first()
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("unknown");
                                let registration = ServiceRegistration {
                                    interface: iface.to_string(),
                                    methods: vec![], // optional; validated at call time
                                };
                                match broker.register_service(conn_id, registration) {
                                    Ok(()) => {
                                        // Send acknowledgment
                                        if let Err(e) = transport.send(
                                            &turnix_ipc_proto::IpcMessage::MethodReturn {
                                                id: 1,
                                                result: Ok(turnix_ipc_proto::IpcValue::Null),
                                            },
                                        ) {
                                            eprintln!("ipc-broker: ack send failed: {e}");
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("ipc-broker: registration error: {e}");
                                        continue;
                                    }
                                }
                            }
                            _ => {
                                eprintln!("ipc-broker: expected Register from new connection");
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("ipc-broker: recv registration failed: {e}");
                        continue;
                    }
                }

                // Run the message loop for this connection.
                if let Err(e) = run_connection(&mut broker, conn_id, transport) {
                    eprintln!("ipc-broker: connection {conn_id} error: {e}");
                }
                broker.disconnect(conn_id);
            }
            Err(e) => {
                eprintln!("ipc-broker: accept failed: {e}");
            }
        }
    }
}

fn run_connection(
    broker: &mut Broker,
    conn_id: u64,
    mut transport: SocketTransport,
) -> Result<(), BrokerError> {
    loop {
        let msg = transport.recv()?;
        let outputs = broker.handle_message(conn_id, msg)?;
        for (_, reply) in outputs {
            transport.send(&reply)?;
        }
    }
}

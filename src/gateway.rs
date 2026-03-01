use std::io::{self, Read};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::matching::MatchingEngine;
use crate::protocol::{
    EXECUTION_REPORT_SIZE, EngineCommand, ProtocolError, decode_message, encode_execution_report,
    message_size,
};
use crate::ring::{self, Consumer, Producer};

#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub listen_addr: SocketAddr,
    pub multicast_addr: SocketAddr,
    /// Must be a power of 2.
    pub ring_capacity: usize,
    pub arena_capacity: u32,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            listen_addr: SocketAddr::new(Ipv4Addr::new(0, 0, 0, 0).into(), 9000),
            multicast_addr: SocketAddr::new(Ipv4Addr::new(239, 1, 1, 1).into(), 9001),
            ring_capacity: 65536,
            arena_capacity: 1_048_576,
        }
    }
}

#[derive(Debug)]
pub enum GatewayError {
    Io(io::Error),
    Protocol(ProtocolError),
}

impl std::fmt::Display for GatewayError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(e) => write!(f, "io error: {e}"),
            Self::Protocol(e) => write!(f, "protocol error: {e}"),
        }
    }
}

impl std::error::Error for GatewayError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Protocol(e) => Some(e),
        }
    }
}

impl From<io::Error> for GatewayError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<ProtocolError> for GatewayError {
    fn from(e: ProtocolError) -> Self {
        Self::Protocol(e)
    }
}

fn now_nanos() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

fn handle_client(
    mut stream: TcpStream,
    producer: &mut Producer<EngineCommand>,
    shutdown: &AtomicBool,
) -> Result<(), GatewayError> {
    let mut type_buf = [0u8; 1];

    loop {
        match stream.read_exact(&mut type_buf) {
            Ok(()) => {}
            Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
            Err(e) if e.kind() == io::ErrorKind::ConnectionReset => break,
            Err(e) => return Err(e.into()),
        }

        let msg_type = type_buf[0];
        let size = message_size(msg_type)?;

        let mut msg_buf = [0u8; 48];
        msg_buf[0] = msg_type;

        if size > 1 {
            match stream.read_exact(&mut msg_buf[1..size]) {
                Ok(()) => {}
                Err(e) if e.kind() == io::ErrorKind::UnexpectedEof => break,
                Err(e) if e.kind() == io::ErrorKind::ConnectionReset => break,
                Err(e) => return Err(e.into()),
            }
        }

        let mut cmd = decode_message(&msg_buf[..size])?;

        if let EngineCommand::NewOrder(ref mut order) = cmd {
            order.timestamp = now_nanos();
        }

        loop {
            match producer.push(cmd) {
                Ok(()) => break,
                Err(ring::Full(returned)) => {
                    cmd = returned;
                    thread::yield_now();
                }
            }
        }
    }

    shutdown.store(true, Ordering::Release);
    Ok(())
}

fn matching_loop(
    mut consumer: Consumer<EngineCommand>,
    mut engine: MatchingEngine,
    udp: UdpSocket,
    multicast_addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
) {
    let mut seq_num: u32 = 0;
    let mut report_buf = [0u8; EXECUTION_REPORT_SIZE];

    loop {
        match consumer.pop() {
            Ok(cmd) => match cmd {
                EngineCommand::NewOrder(order) => {
                    let timestamp = order.timestamp;
                    if let Ok(result) = engine.add_order(order) {
                        for fill in &result.fills {
                            seq_num = seq_num.wrapping_add(1);
                            if encode_execution_report(&mut report_buf, seq_num, fill, timestamp)
                                .is_ok()
                            {
                                let _ = udp.send_to(&report_buf, multicast_addr);
                            }
                        }
                    }
                }
                EngineCommand::CancelOrder { order_id } => {
                    let _ = engine.cancel_order(order_id);
                }
            },
            Err(_empty) => {
                if shutdown.load(Ordering::Acquire) {
                    while let Ok(cmd) = consumer.pop() {
                        match cmd {
                            EngineCommand::NewOrder(order) => {
                                let timestamp = order.timestamp;
                                if let Ok(result) = engine.add_order(order) {
                                    for fill in &result.fills {
                                        seq_num = seq_num.wrapping_add(1);
                                        if encode_execution_report(
                                            &mut report_buf,
                                            seq_num,
                                            fill,
                                            timestamp,
                                        )
                                        .is_ok()
                                        {
                                            let _ = udp.send_to(&report_buf, multicast_addr);
                                        }
                                    }
                                }
                            }
                            EngineCommand::CancelOrder { order_id } => {
                                let _ = engine.cancel_order(order_id);
                            }
                        }
                    }
                    break;
                }
                thread::yield_now();
            }
        }
    }
}

/// Blocks until the TCP client disconnects.
pub fn run(config: GatewayConfig) -> Result<(), GatewayError> {
    let (mut producer, consumer) = ring::ring_buffer::<EngineCommand>(config.ring_capacity);

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_match = Arc::clone(&shutdown);

    let engine = MatchingEngine::with_capacity(config.arena_capacity);

    let udp = UdpSocket::bind("0.0.0.0:0")?;
    udp.set_multicast_ttl_v4(1)?;

    let multicast_addr = config.multicast_addr;

    let match_thread = thread::spawn(move || {
        matching_loop(consumer, engine, udp, multicast_addr, shutdown_match);
    });

    let listener = TcpListener::bind(config.listen_addr)?;
    eprintln!("ferrox: listening on {}", config.listen_addr);

    let (stream, peer) = listener.accept()?;
    eprintln!("ferrox: client connected from {peer}");

    let result = handle_client(stream, &mut producer, &shutdown);

    shutdown.store(true, Ordering::Release);
    eprintln!("ferrox: client disconnected, shutting down");

    match_thread.join().expect("matching thread panicked");

    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::order::{Order, Side};
    use crate::protocol::{self, EXECUTION_REPORT_SIZE, NEW_ORDER_SIZE, encode_new_order};
    use std::io::Write;
    use std::net::TcpStream;
    use std::time::Duration;

    #[test]
    fn engine_command_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<EngineCommand>();
    }

    #[test]
    fn engine_command_size() {
        let size = std::mem::size_of::<EngineCommand>();
        assert!(size <= 64, "EngineCommand too large: {size} bytes");
    }

    #[test]
    fn gateway_config_defaults() {
        let config = GatewayConfig::default();
        assert_eq!(config.listen_addr.port(), 9000);
        assert_eq!(config.multicast_addr.port(), 9001);
        assert_eq!(config.ring_capacity, 65536);
        assert_eq!(config.arena_capacity, 1_048_576);
    }

    #[test]
    fn tcp_to_ring_roundtrip() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();

        let (mut producer, mut consumer) = ring::ring_buffer::<EngineCommand>(64);
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_ref = &shutdown;

        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect(addr).unwrap();
            let order = Order {
                id: 42,
                trader_id: 7,
                side: Side::Bid,
                price: 15005,
                quantity: 100,
                timestamp: 0,
            };
            let mut buf = [0u8; NEW_ORDER_SIZE];
            encode_new_order(&mut buf, &order).unwrap();
            stream.write_all(&buf).unwrap();
        });

        let (stream, _) = listener.accept().unwrap();
        handle_client(stream, &mut producer, shutdown_ref).unwrap();

        client.join().unwrap();

        let cmd = consumer.pop().unwrap();
        match cmd {
            EngineCommand::NewOrder(order) => {
                assert_eq!(order.id, 42);
                assert_eq!(order.trader_id, 7);
                assert_eq!(order.side, Side::Bid);
                assert_eq!(order.price, 15005);
                assert_eq!(order.quantity, 100);
                assert!(order.timestamp > 0, "timestamp should be assigned");
            }
            _ => panic!("expected NewOrder"),
        }
    }

    #[test]
    fn full_pipeline_integration() {
        let tcp_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let tcp_addr = tcp_listener.local_addr().unwrap();

        let udp_recv = UdpSocket::bind("127.0.0.1:0").unwrap();
        let udp_recv_addr = udp_recv.local_addr().unwrap();
        udp_recv
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();

        let (mut producer, consumer) = ring::ring_buffer::<EngineCommand>(64);
        let shutdown = Arc::new(AtomicBool::new(false));
        let shutdown_match = Arc::clone(&shutdown);

        let engine = MatchingEngine::with_capacity(1024);
        let udp_send = UdpSocket::bind("0.0.0.0:0").unwrap();

        let match_thread = thread::spawn(move || {
            matching_loop(consumer, engine, udp_send, udp_recv_addr, shutdown_match);
        });

        let client = thread::spawn(move || {
            let mut stream = TcpStream::connect(tcp_addr).unwrap();

            let ask = Order {
                id: 1,
                trader_id: 10,
                side: Side::Ask,
                price: 100,
                quantity: 50,
                timestamp: 0,
            };
            let mut buf = [0u8; NEW_ORDER_SIZE];
            encode_new_order(&mut buf, &ask).unwrap();
            stream.write_all(&buf).unwrap();

            let bid = Order {
                id: 2,
                trader_id: 20,
                side: Side::Bid,
                price: 100,
                quantity: 50,
                timestamp: 0,
            };
            encode_new_order(&mut buf, &bid).unwrap();
            stream.write_all(&buf).unwrap();

            thread::sleep(Duration::from_millis(50));
        });

        let (stream, _) = tcp_listener.accept().unwrap();
        let shutdown_ref = &shutdown;
        handle_client(stream, &mut producer, shutdown_ref).unwrap();
        shutdown.store(true, Ordering::Release);

        client.join().unwrap();
        match_thread.join().unwrap();

        let mut report_buf = [0u8; EXECUTION_REPORT_SIZE];
        let (n, _) = udp_recv.recv_from(&mut report_buf).unwrap();
        assert_eq!(n, EXECUTION_REPORT_SIZE);

        let report = protocol::decode_execution_report(&report_buf).unwrap();
        assert_eq!(report.seq_num, 1);
        assert_eq!(report.taker_order_id, 2);
        assert_eq!(report.maker_order_id, 1);
        assert_eq!(report.price, 100);
        assert_eq!(report.quantity, 50);
        assert!(report.timestamp > 0);
    }
}

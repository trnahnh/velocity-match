use std::net::{Ipv4Addr, UdpSocket};

use ferrox::protocol::{self, EXECUTION_REPORT_SIZE};

fn main() {
    let socket = UdpSocket::bind("0.0.0.0:9001").expect("failed to bind UDP socket");

    socket
        .join_multicast_v4(&Ipv4Addr::new(239, 1, 1, 1), &Ipv4Addr::UNSPECIFIED)
        .expect("failed to join multicast group");

    eprintln!("subscriber: listening for execution reports on 239.1.1.1:9001");

    let mut buf = [0u8; EXECUTION_REPORT_SIZE];
    let mut expected_seq: u32 = 1;

    loop {
        let (n, src) = match socket.recv_from(&mut buf) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("subscriber: recv error: {e}");
                continue;
            }
        };

        if n < EXECUTION_REPORT_SIZE {
            eprintln!("subscriber: short packet ({n} bytes) from {src}");
            continue;
        }

        let report = match protocol::decode_execution_report(&buf) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("subscriber: decode error: {e}");
                continue;
            }
        };

        if report.seq_num != expected_seq {
            let gap = report.seq_num.wrapping_sub(expected_seq);
            eprintln!(
                "subscriber: GAP detected — expected seq {expected_seq}, got {}, missing {gap} report(s)",
                report.seq_num
            );
        }
        expected_seq = report.seq_num.wrapping_add(1);

        println!(
            "seq={} taker={} maker={} price={} qty={} ts={}",
            report.seq_num,
            report.taker_order_id,
            report.maker_order_id,
            report.price,
            report.quantity,
            report.timestamp,
        );
    }
}

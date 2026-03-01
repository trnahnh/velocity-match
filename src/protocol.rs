use crate::order::{Order, Side};

pub const MSG_NEW_ORDER: u8 = 0x01;
pub const MSG_CANCEL_ORDER: u8 = 0x02;
pub const MSG_EXECUTION_REPORT: u8 = 0x03;

pub const NEW_ORDER_SIZE: usize = 40;
pub const CANCEL_ORDER_SIZE: usize = 16;
pub const EXECUTION_REPORT_SIZE: usize = 48;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EngineCommand {
    NewOrder(Order),
    CancelOrder { order_id: u64 },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionReport {
    pub seq_num: u32,
    pub taker_order_id: u64,
    pub maker_order_id: u64,
    pub price: i64,
    pub quantity: u64,
    pub timestamp: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProtocolError {
    BufferTooShort,
    UnknownMessageType(u8),
    InvalidSide(u8),
    ZeroQuantity,
}

impl std::fmt::Display for ProtocolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BufferTooShort => write!(f, "buffer too short"),
            Self::UnknownMessageType(t) => write!(f, "unknown message type: 0x{t:02x}"),
            Self::InvalidSide(s) => write!(f, "invalid side: {s}"),
            Self::ZeroQuantity => write!(f, "zero quantity"),
        }
    }
}

impl std::error::Error for ProtocolError {}

fn read_u8(buf: &[u8], offset: usize) -> Result<u8, ProtocolError> {
    buf.get(offset)
        .copied()
        .ok_or(ProtocolError::BufferTooShort)
}

fn read_u32(buf: &[u8], offset: usize) -> Result<u32, ProtocolError> {
    let bytes: [u8; 4] = buf
        .get(offset..offset + 4)
        .ok_or(ProtocolError::BufferTooShort)?
        .try_into()
        .map_err(|_| ProtocolError::BufferTooShort)?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_u64(buf: &[u8], offset: usize) -> Result<u64, ProtocolError> {
    let bytes: [u8; 8] = buf
        .get(offset..offset + 8)
        .ok_or(ProtocolError::BufferTooShort)?
        .try_into()
        .map_err(|_| ProtocolError::BufferTooShort)?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_i64(buf: &[u8], offset: usize) -> Result<i64, ProtocolError> {
    let bytes: [u8; 8] = buf
        .get(offset..offset + 8)
        .ok_or(ProtocolError::BufferTooShort)?
        .try_into()
        .map_err(|_| ProtocolError::BufferTooShort)?;
    Ok(i64::from_le_bytes(bytes))
}

fn write_u8(buf: &mut [u8], offset: usize, val: u8) -> Result<(), ProtocolError> {
    *buf.get_mut(offset).ok_or(ProtocolError::BufferTooShort)? = val;
    Ok(())
}

fn write_u32(buf: &mut [u8], offset: usize, val: u32) -> Result<(), ProtocolError> {
    let bytes = val.to_le_bytes();
    buf.get_mut(offset..offset + 4)
        .ok_or(ProtocolError::BufferTooShort)?
        .copy_from_slice(&bytes);
    Ok(())
}

fn write_u64(buf: &mut [u8], offset: usize, val: u64) -> Result<(), ProtocolError> {
    let bytes = val.to_le_bytes();
    buf.get_mut(offset..offset + 8)
        .ok_or(ProtocolError::BufferTooShort)?
        .copy_from_slice(&bytes);
    Ok(())
}

fn write_i64(buf: &mut [u8], offset: usize, val: i64) -> Result<(), ProtocolError> {
    let bytes = val.to_le_bytes();
    buf.get_mut(offset..offset + 8)
        .ok_or(ProtocolError::BufferTooShort)?
        .copy_from_slice(&bytes);
    Ok(())
}

fn decode_side(val: u8) -> Result<Side, ProtocolError> {
    match val {
        0 => Ok(Side::Bid),
        1 => Ok(Side::Ask),
        _ => Err(ProtocolError::InvalidSide(val)),
    }
}

fn encode_side(side: Side) -> u8 {
    match side {
        Side::Bid => 0,
        Side::Ask => 1,
    }
}

pub fn decode_new_order(buf: &[u8]) -> Result<Order, ProtocolError> {
    if buf.len() < NEW_ORDER_SIZE {
        return Err(ProtocolError::BufferTooShort);
    }

    let side = decode_side(read_u8(buf, 1)?)?;
    let order_id = read_u64(buf, 8)?;
    let trader_id = read_u64(buf, 16)?;
    let price = read_i64(buf, 24)?;
    let quantity = read_u64(buf, 32)?;

    if quantity == 0 {
        return Err(ProtocolError::ZeroQuantity);
    }

    Ok(Order {
        id: order_id,
        side,
        trader_id,
        price,
        quantity,
        timestamp: 0,
    })
}

pub fn encode_new_order(buf: &mut [u8], order: &Order) -> Result<usize, ProtocolError> {
    if buf.len() < NEW_ORDER_SIZE {
        return Err(ProtocolError::BufferTooShort);
    }

    buf[..NEW_ORDER_SIZE].fill(0);

    write_u8(buf, 0, MSG_NEW_ORDER)?;
    write_u8(buf, 1, encode_side(order.side))?;
    write_u64(buf, 8, order.id)?;
    write_u64(buf, 16, order.trader_id)?;
    write_i64(buf, 24, order.price)?;
    write_u64(buf, 32, order.quantity)?;

    Ok(NEW_ORDER_SIZE)
}

pub fn decode_cancel_order(buf: &[u8]) -> Result<u64, ProtocolError> {
    if buf.len() < CANCEL_ORDER_SIZE {
        return Err(ProtocolError::BufferTooShort);
    }

    read_u64(buf, 8)
}

pub fn encode_cancel_order(buf: &mut [u8], order_id: u64) -> Result<usize, ProtocolError> {
    if buf.len() < CANCEL_ORDER_SIZE {
        return Err(ProtocolError::BufferTooShort);
    }

    buf[..CANCEL_ORDER_SIZE].fill(0);

    write_u8(buf, 0, MSG_CANCEL_ORDER)?;
    write_u64(buf, 8, order_id)?;

    Ok(CANCEL_ORDER_SIZE)
}

pub fn decode_message(buf: &[u8]) -> Result<EngineCommand, ProtocolError> {
    let msg_type = read_u8(buf, 0)?;
    match msg_type {
        MSG_NEW_ORDER => Ok(EngineCommand::NewOrder(decode_new_order(buf)?)),
        MSG_CANCEL_ORDER => Ok(EngineCommand::CancelOrder {
            order_id: decode_cancel_order(buf)?,
        }),
        other => Err(ProtocolError::UnknownMessageType(other)),
    }
}

pub fn message_size(msg_type: u8) -> Result<usize, ProtocolError> {
    match msg_type {
        MSG_NEW_ORDER => Ok(NEW_ORDER_SIZE),
        MSG_CANCEL_ORDER => Ok(CANCEL_ORDER_SIZE),
        _ => Err(ProtocolError::UnknownMessageType(msg_type)),
    }
}

pub fn encode_execution_report(
    buf: &mut [u8],
    seq_num: u32,
    fill: &crate::matching::Fill,
    timestamp: u64,
) -> Result<usize, ProtocolError> {
    if buf.len() < EXECUTION_REPORT_SIZE {
        return Err(ProtocolError::BufferTooShort);
    }

    buf[..EXECUTION_REPORT_SIZE].fill(0);

    write_u8(buf, 0, MSG_EXECUTION_REPORT)?;
    write_u32(buf, 4, seq_num)?;
    write_u64(buf, 8, fill.taker_order_id)?;
    write_u64(buf, 16, fill.maker_order_id)?;
    write_i64(buf, 24, fill.price)?;
    write_u64(buf, 32, fill.quantity)?;
    write_u64(buf, 40, timestamp)?;

    Ok(EXECUTION_REPORT_SIZE)
}

pub fn decode_execution_report(buf: &[u8]) -> Result<ExecutionReport, ProtocolError> {
    if buf.len() < EXECUTION_REPORT_SIZE {
        return Err(ProtocolError::BufferTooShort);
    }

    Ok(ExecutionReport {
        seq_num: read_u32(buf, 4)?,
        taker_order_id: read_u64(buf, 8)?,
        maker_order_id: read_u64(buf, 16)?,
        price: read_i64(buf, 24)?,
        quantity: read_u64(buf, 32)?,
        timestamp: read_u64(buf, 40)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::matching::Fill;

    #[test]
    fn roundtrip_new_order_bid() {
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

        let decoded = decode_new_order(&buf).unwrap();
        assert_eq!(decoded.id, 42);
        assert_eq!(decoded.trader_id, 7);
        assert_eq!(decoded.side, Side::Bid);
        assert_eq!(decoded.price, 15005);
        assert_eq!(decoded.quantity, 100);
        assert_eq!(decoded.timestamp, 0);
    }

    #[test]
    fn roundtrip_new_order_ask() {
        let order = Order {
            id: 99,
            trader_id: 3,
            side: Side::Ask,
            price: -500,
            quantity: 1,
            timestamp: 0,
        };

        let mut buf = [0u8; NEW_ORDER_SIZE];
        encode_new_order(&mut buf, &order).unwrap();

        let decoded = decode_new_order(&buf).unwrap();
        assert_eq!(decoded.side, Side::Ask);
        assert_eq!(decoded.price, -500);
    }

    #[test]
    fn roundtrip_cancel_order() {
        let mut buf = [0u8; CANCEL_ORDER_SIZE];
        encode_cancel_order(&mut buf, 12345).unwrap();

        assert_eq!(buf[0], MSG_CANCEL_ORDER);
        let order_id = decode_cancel_order(&buf).unwrap();
        assert_eq!(order_id, 12345);
    }

    #[test]
    fn roundtrip_execution_report() {
        let fill = Fill {
            taker_order_id: 10,
            maker_order_id: 20,
            price: 9999,
            quantity: 50,
            maker_fully_filled: true,
        };

        let mut buf = [0u8; EXECUTION_REPORT_SIZE];
        encode_execution_report(&mut buf, 1, &fill, 123_456_789).unwrap();

        let report = decode_execution_report(&buf).unwrap();
        assert_eq!(report.seq_num, 1);
        assert_eq!(report.taker_order_id, 10);
        assert_eq!(report.maker_order_id, 20);
        assert_eq!(report.price, 9999);
        assert_eq!(report.quantity, 50);
        assert_eq!(report.timestamp, 123_456_789);
    }

    #[test]
    fn side_mapping_bid_is_zero_ask_is_one() {
        assert_eq!(encode_side(Side::Bid), 0);
        assert_eq!(encode_side(Side::Ask), 1);
        assert_eq!(decode_side(0).unwrap(), Side::Bid);
        assert_eq!(decode_side(1).unwrap(), Side::Ask);
    }

    #[test]
    fn new_order_buffer_too_short() {
        let buf = [0u8; NEW_ORDER_SIZE - 1];
        assert_eq!(decode_new_order(&buf), Err(ProtocolError::BufferTooShort));
    }

    #[test]
    fn cancel_order_buffer_too_short() {
        let buf = [0u8; CANCEL_ORDER_SIZE - 1];
        assert_eq!(
            decode_cancel_order(&buf),
            Err(ProtocolError::BufferTooShort)
        );
    }

    #[test]
    fn execution_report_buffer_too_short() {
        let buf = [0u8; EXECUTION_REPORT_SIZE - 1];
        assert_eq!(
            decode_execution_report(&buf),
            Err(ProtocolError::BufferTooShort)
        );
    }

    #[test]
    fn unknown_message_type() {
        let buf = [0xFF; NEW_ORDER_SIZE];
        assert_eq!(
            decode_message(&buf),
            Err(ProtocolError::UnknownMessageType(0xFF))
        );
    }

    #[test]
    fn invalid_side() {
        let mut buf = [0u8; NEW_ORDER_SIZE];
        buf[0] = MSG_NEW_ORDER;
        buf[1] = 2;
        buf[32..40].copy_from_slice(&100u64.to_le_bytes());
        assert_eq!(decode_new_order(&buf), Err(ProtocolError::InvalidSide(2)));
    }

    #[test]
    fn zero_quantity_rejected() {
        let mut buf = [0u8; NEW_ORDER_SIZE];
        buf[0] = MSG_NEW_ORDER;
        buf[1] = 0;
        assert_eq!(decode_new_order(&buf), Err(ProtocolError::ZeroQuantity));
    }

    #[test]
    fn encode_new_order_buffer_too_short() {
        let order = Order {
            id: 1,
            trader_id: 1,
            side: Side::Bid,
            price: 100,
            quantity: 10,
            timestamp: 0,
        };
        let mut buf = [0u8; NEW_ORDER_SIZE - 1];
        assert_eq!(
            encode_new_order(&mut buf, &order),
            Err(ProtocolError::BufferTooShort)
        );
    }

    #[test]
    fn encode_execution_report_buffer_too_short() {
        let fill = Fill {
            taker_order_id: 1,
            maker_order_id: 2,
            price: 100,
            quantity: 10,
            maker_fully_filled: true,
        };
        let mut buf = [0u8; EXECUTION_REPORT_SIZE - 1];
        assert_eq!(
            encode_execution_report(&mut buf, 1, &fill, 0),
            Err(ProtocolError::BufferTooShort)
        );
    }

    #[test]
    fn decode_message_dispatches_new_order() {
        let order = Order {
            id: 5,
            trader_id: 3,
            side: Side::Ask,
            price: 200,
            quantity: 50,
            timestamp: 0,
        };

        let mut buf = [0u8; NEW_ORDER_SIZE];
        encode_new_order(&mut buf, &order).unwrap();

        let cmd = decode_message(&buf).unwrap();
        match cmd {
            EngineCommand::NewOrder(o) => {
                assert_eq!(o.id, 5);
                assert_eq!(o.side, Side::Ask);
            }
            _ => panic!("expected NewOrder"),
        }
    }

    #[test]
    fn decode_message_dispatches_cancel() {
        let mut buf = [0u8; CANCEL_ORDER_SIZE];
        encode_cancel_order(&mut buf, 999).unwrap();

        let cmd = decode_message(&buf).unwrap();
        assert_eq!(cmd, EngineCommand::CancelOrder { order_id: 999 });
    }

    #[test]
    fn negative_price_roundtrips() {
        let order = Order {
            id: 1,
            trader_id: 1,
            side: Side::Bid,
            price: i64::MIN,
            quantity: 1,
            timestamp: 0,
        };

        let mut buf = [0u8; NEW_ORDER_SIZE];
        encode_new_order(&mut buf, &order).unwrap();
        let decoded = decode_new_order(&buf).unwrap();
        assert_eq!(decoded.price, i64::MIN);
    }

    #[test]
    fn max_values_roundtrip() {
        let order = Order {
            id: u64::MAX,
            trader_id: u64::MAX,
            side: Side::Ask,
            price: i64::MAX,
            quantity: u64::MAX,
            timestamp: 0,
        };

        let mut buf = [0u8; NEW_ORDER_SIZE];
        encode_new_order(&mut buf, &order).unwrap();
        let decoded = decode_new_order(&buf).unwrap();
        assert_eq!(decoded.id, u64::MAX);
        assert_eq!(decoded.trader_id, u64::MAX);
        assert_eq!(decoded.price, i64::MAX);
        assert_eq!(decoded.quantity, u64::MAX);
    }

    #[test]
    fn reserved_bytes_ignored() {
        let order = Order {
            id: 1,
            trader_id: 1,
            side: Side::Bid,
            price: 100,
            quantity: 10,
            timestamp: 0,
        };

        let mut buf = [0u8; NEW_ORDER_SIZE];
        encode_new_order(&mut buf, &order).unwrap();

        buf[2..8].fill(0xFF);

        let decoded = decode_new_order(&buf).unwrap();
        assert_eq!(decoded.id, 1);
        assert_eq!(decoded.quantity, 10);
    }

    #[test]
    fn message_size_lookup() {
        assert_eq!(message_size(MSG_NEW_ORDER).unwrap(), NEW_ORDER_SIZE);
        assert_eq!(message_size(MSG_CANCEL_ORDER).unwrap(), CANCEL_ORDER_SIZE);
        assert!(message_size(0xFF).is_err());
    }

    #[test]
    fn empty_buffer_returns_error() {
        let buf: &[u8] = &[];
        assert_eq!(decode_message(buf), Err(ProtocolError::BufferTooShort));
    }
}

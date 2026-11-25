//! Modbus RTU framing.
//!
//! Unlike the Node build (which asked `modbus-serial` to open a *second*
//! connection to a port the app already held open — a guaranteed
//! ERROR_ACCESS_DENIED on Windows), this speaks RTU over the session's
//! existing handle: build frame, write, drain the reply out of the session's
//! scratch buffer.

use serde::{Deserialize, Serialize};

pub const RESPONSE_TIMEOUT_MS: u64 = 2000;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModbusRequest {
    pub tab_id: String,
    pub slave_id: u8,
    pub function_code: u8,
    pub start_address: u16,
    pub quantity: u16,
    #[serde(default)]
    pub write_value: String,
}

#[derive(Debug, Serialize, Default)]
pub struct ModbusReply {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Vec<i64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl ModbusReply {
    pub fn err(msg: impl Into<String>) -> Self {
        Self { data: None, error: Some(msg.into()) }
    }
    pub fn ok(data: Vec<i64>) -> Self {
        Self { data: Some(data), error: None }
    }
}

/// Standard Modbus CRC-16 (poly 0xA001, init 0xFFFF), little-endian on the wire.
pub fn crc16(bytes: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in bytes {
        crc ^= b as u16;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    crc
}

fn finish(mut frame: Vec<u8>) -> Vec<u8> {
    let crc = crc16(&frame);
    frame.push((crc & 0xff) as u8);
    frame.push((crc >> 8) as u8);
    frame
}

fn parse_coils(s: &str) -> Vec<bool> {
    s.split(',')
        .map(|p| p.trim().parse::<i64>().unwrap_or(0) != 0)
        .collect()
}

fn parse_regs(s: &str) -> Vec<u16> {
    s.split(',')
        .map(|p| p.trim().parse::<i64>().unwrap_or(0) as u16)
        .collect()
}

pub struct Encoded {
    pub frame: Vec<u8>,
    /// Expected byte length of a non-exception response.
    pub expect_len: usize,
    /// Values echoed back to the UI for write function codes.
    pub echo: Option<Vec<i64>>,
}

pub fn encode(req: &ModbusRequest) -> Result<Encoded, String> {
    let id = req.slave_id;
    let fc = req.function_code;
    let addr = req.start_address;
    let qty = req.quantity;

    let head = |fc: u8| vec![id, fc, (addr >> 8) as u8, (addr & 0xff) as u8];

    match fc {
        // Read coils / discrete inputs: ceil(qty / 8) data bytes.
        1 | 2 => {
            if qty == 0 || qty > 2000 {
                return Err("Quantity must be 1-2000 for bit reads".into());
            }
            let mut f = head(fc);
            f.push((qty >> 8) as u8);
            f.push((qty & 0xff) as u8);
            Ok(Encoded {
                frame: finish(f),
                expect_len: 5 + qty.div_ceil(8) as usize,
                echo: None,
            })
        }
        // Read holding / input registers.
        3 | 4 => {
            if qty == 0 || qty > 125 {
                return Err("Quantity must be 1-125 for register reads".into());
            }
            let mut f = head(fc);
            f.push((qty >> 8) as u8);
            f.push((qty & 0xff) as u8);
            Ok(Encoded {
                frame: finish(f),
                expect_len: 5 + (qty as usize * 2),
                echo: None,
            })
        }
        // Write single coil - the response echoes the request.
        5 => {
            let on = req.write_value.trim().parse::<i64>().unwrap_or(0) != 0;
            let mut f = head(fc);
            f.push(if on { 0xFF } else { 0x00 });
            f.push(0x00);
            Ok(Encoded { frame: finish(f), expect_len: 8, echo: Some(vec![on as i64]) })
        }
        // Write single register - the response echoes the request.
        6 => {
            let val = req.write_value.trim().parse::<i64>().unwrap_or(0) as u16;
            let mut f = head(fc);
            f.push((val >> 8) as u8);
            f.push((val & 0xff) as u8);
            Ok(Encoded { frame: finish(f), expect_len: 8, echo: Some(vec![val as i64]) })
        }
        // Write multiple coils.
        15 => {
            let bits = parse_coils(&req.write_value);
            if bits.is_empty() || bits.len() > 1968 {
                return Err("Provide 1-1968 comma-separated coil values".into());
            }
            let byte_count = bits.len().div_ceil(8);
            let mut packed = vec![0u8; byte_count];
            for (i, &bit) in bits.iter().enumerate() {
                if bit {
                    packed[i / 8] |= 1 << (i % 8);
                }
            }
            let mut f = head(fc);
            f.push((bits.len() >> 8) as u8);
            f.push((bits.len() & 0xff) as u8);
            f.push(byte_count as u8);
            f.extend_from_slice(&packed);
            Ok(Encoded {
                frame: finish(f),
                expect_len: 8,
                echo: Some(bits.iter().map(|&b| b as i64).collect()),
            })
        }
        // Write multiple registers.
        16 => {
            let regs = parse_regs(&req.write_value);
            if regs.is_empty() || regs.len() > 123 {
                return Err("Provide 1-123 comma-separated register values".into());
            }
            let mut f = head(fc);
            f.push((regs.len() >> 8) as u8);
            f.push((regs.len() & 0xff) as u8);
            f.push((regs.len() * 2) as u8);
            for r in &regs {
                f.push((r >> 8) as u8);
                f.push((r & 0xff) as u8);
            }
            Ok(Encoded {
                frame: finish(f),
                expect_len: 8,
                echo: Some(regs.iter().map(|&r| r as i64).collect()),
            })
        }
        other => Err(format!("Function code {other} not supported")),
    }
}

fn exception_text(code: u8) -> &'static str {
    match code {
        0x01 => "Illegal function",
        0x02 => "Illegal data address",
        0x03 => "Illegal data value",
        0x04 => "Slave device failure",
        0x05 => "Acknowledge (still processing)",
        0x06 => "Slave device busy",
        0x08 => "Memory parity error",
        0x0A => "Gateway path unavailable",
        0x0B => "Gateway target failed to respond",
        _ => "Unknown exception",
    }
}

/// Bytes still missing before `buf` can be decoded; `0` means a full frame.
pub fn needed(buf: &[u8], expect_len: usize) -> usize {
    if buf.len() < 2 {
        return 2 - buf.len();
    }
    // Exception responses set the high bit of the function code and are 5 bytes.
    if buf[1] & 0x80 != 0 {
        return 5usize.saturating_sub(buf.len());
    }
    expect_len.saturating_sub(buf.len())
}

pub fn decode(req: &ModbusRequest, enc: &Encoded, buf: &[u8]) -> ModbusReply {
    if buf.len() < 5 {
        return ModbusReply::err("Short response from slave");
    }
    if buf[0] != req.slave_id {
        return ModbusReply::err(format!(
            "Response from slave {} (expected {})",
            buf[0], req.slave_id
        ));
    }

    let is_exception = buf[1] & 0x80 != 0;
    let body_len = if is_exception { 3 } else { enc.expect_len - 2 };
    if buf.len() < body_len + 2 {
        return ModbusReply::err("Truncated response");
    }
    let expected_crc = crc16(&buf[..body_len]);
    let actual_crc = u16::from(buf[body_len]) | (u16::from(buf[body_len + 1]) << 8);
    if expected_crc != actual_crc {
        return ModbusReply::err("CRC mismatch");
    }

    if is_exception {
        let code = buf[2];
        return ModbusReply::err(format!(
            "Modbus exception {:#04x}: {}",
            code,
            exception_text(code)
        ));
    }

    match req.function_code {
        1 | 2 => {
            let count = buf[2] as usize;
            let data = &buf[3..(3 + count).min(buf.len())];
            let mut out = Vec::with_capacity(req.quantity as usize);
            for i in 0..req.quantity as usize {
                let byte = data.get(i / 8).copied().unwrap_or(0);
                out.push(((byte >> (i % 8)) & 1) as i64);
            }
            ModbusReply::ok(out)
        }
        3 | 4 => {
            let count = buf[2] as usize;
            let data = &buf[3..(3 + count).min(buf.len())];
            let out = data
                .chunks_exact(2)
                .map(|c| ((u16::from(c[0]) << 8) | u16::from(c[1])) as i64)
                .collect();
            ModbusReply::ok(out)
        }
        // Writes: the slave echoed our request, so report what we sent.
        _ => ModbusReply::ok(enc.echo.clone().unwrap_or_default()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc_matches_reference_frame() {
        // Classic example frame: slave 1, FC 3, addr 0, qty 10 -> CRC 0xCDC5.
        assert_eq!(crc16(&[0x01, 0x03, 0x00, 0x00, 0x00, 0x0A]), 0xCDC5);
    }

    #[test]
    fn read_holding_registers_roundtrip() {
        let req = ModbusRequest {
            tab_id: "t".into(),
            slave_id: 1,
            function_code: 3,
            start_address: 0,
            quantity: 2,
            write_value: String::new(),
        };
        let enc = encode(&req).unwrap();
        assert_eq!(enc.expect_len, 9);

        let mut resp = vec![0x01, 0x03, 0x04, 0x00, 0x2A, 0x01, 0x00];
        let crc = crc16(&resp);
        resp.push((crc & 0xff) as u8);
        resp.push((crc >> 8) as u8);

        assert_eq!(needed(&resp, enc.expect_len), 0);
        assert_eq!(decode(&req, &enc, &resp).data.unwrap(), vec![42, 256]);
    }

    #[test]
    fn exception_is_surfaced() {
        let req = ModbusRequest {
            tab_id: "t".into(),
            slave_id: 1,
            function_code: 3,
            start_address: 0,
            quantity: 2,
            write_value: String::new(),
        };
        let enc = encode(&req).unwrap();
        let mut resp = vec![0x01, 0x83, 0x02];
        let crc = crc16(&resp);
        resp.push((crc & 0xff) as u8);
        resp.push((crc >> 8) as u8);

        let reply = decode(&req, &enc, &resp);
        assert!(reply.error.unwrap().contains("Illegal data address"));
    }
}

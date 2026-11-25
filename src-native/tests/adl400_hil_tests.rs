//! Hardware-in-the-Loop (HIL) Test Suite for Acrel ADL400 3-Phase Energy Meter.
//!
//! Simulates physical bus timing, byte pacing, fragmentation, noisy transmission lines,
//! multi-drop RS485 bus topologies, and high-frequency polling stress.

use seriala_native::adl400::{Adl400Simulator, Adl400Telemetry, REG_VOLTAGE_A};
use seriala_native::modbus::{decode, encode, needed, ModbusRequest};
use seriala_native::serial::Scratch;

#[test]
fn test_hil_packet_fragmentation_and_chunking() {
    let mut meter = Adl400Simulator::new(1, Adl400Telemetry::default());
    let scratch = Scratch::default();

    let req = ModbusRequest {
        tab_id: "hil_tab".into(),
        slave_id: 1,
        function_code: 3,
        start_address: REG_VOLTAGE_A,
        quantity: 6, // 6 voltage registers
        write_value: String::new(),
    };

    let enc = encode(&req).unwrap();
    let full_resp = meter.handle_raw_frame(&enc.frame).unwrap();
    assert_eq!(full_resp.len(), 5 + (6 * 2)); // 17 bytes

    // Simulate physical serial reception arriving in 3 separate fragmented byte chunks
    let chunk1 = &full_resp[0..4];
    let chunk2 = &full_resp[4..11];
    let chunk3 = &full_resp[11..];

    // Chunk 1 arrives
    scratch.push(chunk1);
    let buf1 = scratch.snapshot();
    assert!(needed(&buf1, enc.expect_len) > 0, "Frame should be incomplete after chunk 1");

    // Chunk 2 arrives
    scratch.push(chunk2);
    let buf2 = scratch.snapshot();
    assert!(needed(&buf2, enc.expect_len) > 0, "Frame should be incomplete after chunk 2");

    // Chunk 3 arrives
    scratch.push(chunk3);
    let buf3 = scratch.snapshot();
    assert_eq!(needed(&buf3, enc.expect_len), 0, "Frame must be complete after chunk 3");

    // Decode reconstructed frame
    let reply = decode(&req, &enc, &buf3);
    assert!(reply.error.is_none());
    assert_eq!(reply.data.unwrap().len(), 6);
}

#[test]
fn test_hil_noise_and_crc_corruption_recovery() {
    let mut meter = Adl400Simulator::new(1, Adl400Telemetry::default());
    let req = ModbusRequest {
        tab_id: "hil_tab".into(),
        slave_id: 1,
        function_code: 3,
        start_address: REG_VOLTAGE_A,
        quantity: 2,
        write_value: String::new(),
    };

    let enc = encode(&req).unwrap();
    let mut resp = meter.handle_raw_frame(&enc.frame).unwrap();

    // Corrupt one data byte to simulate line noise / EMI spike
    resp[3] ^= 0xFF;

    let reply = decode(&req, &enc, &resp);
    assert!(reply.error.is_some(), "Corrupt CRC must be detected");
    assert!(reply.error.unwrap().contains("CRC mismatch"));

    // Next query with clean line
    let clean_resp = meter.handle_raw_frame(&enc.frame).unwrap();
    let clean_reply = decode(&req, &enc, &clean_resp);
    assert!(clean_reply.error.is_none(), "Clean frame should decode successfully");
}

#[test]
fn test_hil_multidrop_rs485_bus_isolation() {
    // 3 ADL400 meters on the same RS485 bus
    let mut meter_slave1 = Adl400Simulator::new(1, Adl400Telemetry { ua: 230.0, ..Default::default() });
    let mut meter_slave2 = Adl400Simulator::new(2, Adl400Telemetry { ua: 235.0, ..Default::default() });
    let mut meter_slave3 = Adl400Simulator::new(3, Adl400Telemetry { ua: 240.0, ..Default::default() });

    // Broadcast request to Slave 2
    let req2 = ModbusRequest {
        tab_id: "hil_tab".into(),
        slave_id: 2,
        function_code: 3,
        start_address: REG_VOLTAGE_A,
        quantity: 1,
        write_value: String::new(),
    };

    let enc2 = encode(&req2).unwrap();

    // Meter 1 ignores
    assert!(meter_slave1.handle_raw_frame(&enc2.frame).is_none());
    // Meter 3 ignores
    assert!(meter_slave3.handle_raw_frame(&enc2.frame).is_none());
    // Meter 2 responds
    let resp2 = meter_slave2.handle_raw_frame(&enc2.frame).unwrap();
    let reply2 = decode(&req2, &enc2, &resp2);

    let ua2 = reply2.data.unwrap()[0] as f64 * 0.1;
    assert_eq!(ua2, 235.0, "Slave 2 voltage should match its configuration");
}

#[test]
fn test_hil_continuous_high_speed_polling_stress() {
    let mut meter = Adl400Simulator::new(1, Adl400Telemetry::default());
    let req = ModbusRequest {
        tab_id: "hil_tab".into(),
        slave_id: 1,
        function_code: 3,
        start_address: 0x0039,
        quantity: 10,
        write_value: String::new(),
    };

    let enc = encode(&req).unwrap();

    // Run 500 consecutive rapid poll cycles
    for i in 0..500 {
        // Vary telemetry slightly to simulate dynamic grid load
        meter.telemetry.ua = 230.0 + ((i % 10) as f64 * 0.1);
        meter.sync_registers_from_telemetry();

        let resp = meter.handle_raw_frame(&enc.frame).expect("Meter response");
        let reply = decode(&req, &enc, &resp);
        assert!(reply.error.is_none());
        assert_eq!(reply.data.unwrap().len(), 10);
    }

    assert_eq!(meter.rx_count, 500);
    assert_eq!(meter.tx_count, 500);
    assert_eq!(meter.error_count, 0);
}

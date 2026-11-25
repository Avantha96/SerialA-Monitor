//! Software-in-the-Loop (SIL) Test Suite for Acrel ADL400 3-Phase Energy Meter.

use seriala_native::adl400::{
    Adl400Simulator, Adl400Telemetry, REG_CT_RATIO, REG_PT_RATIO,
    REG_TOTAL_ACTIVE_ENERGY_HI, REG_VOLTAGE_A,
};
use seriala_native::modbus::{crc16, decode, encode, ModbusRequest};

#[test]
fn test_sil_read_3phase_voltages_and_currents() {
    let telemetry = Adl400Telemetry {
        ua: 230.5,
        ub: 230.1,
        uc: 229.9,
        uab: 399.2,
        ubc: 398.7,
        uca: 399.0,
        ia: 5.25,
        ib: 5.20,
        ic: 5.30,
        ..Default::default()
    };

    let mut meter = Adl400Simulator::new(1, telemetry);

    // Request 9 registers starting from REG_VOLTAGE_A (0x0039): Ua, Ub, Uc, Uab, Ubc, Uca, Ia, Ib, Ic
    let req = ModbusRequest {
        tab_id: "sil_tab".into(),
        slave_id: 1,
        function_code: 3,
        start_address: REG_VOLTAGE_A,
        quantity: 9,
        write_value: String::new(),
    };

    let enc = encode(&req).expect("Encode request");
    let resp_bytes = meter
        .handle_raw_frame(&enc.frame)
        .expect("Meter generated response");

    let reply = decode(&req, &enc, &resp_bytes);
    assert!(reply.error.is_none(), "Decode error: {:?}", reply.error);

    let data = reply.data.expect("Data registers");
    assert_eq!(data.len(), 9);

    // Scale factors: Voltage is 0.1 V, Current is 0.01 A
    let ua = data[0] as f64 * 0.1;
    let ub = data[1] as f64 * 0.1;
    let uc = data[2] as f64 * 0.1;
    let uab = data[3] as f64 * 0.1;
    let ubc = data[4] as f64 * 0.1;
    let uca = data[5] as f64 * 0.1;

    let ia = data[6] as f64 * 0.01;
    let ib = data[7] as f64 * 0.01;
    let ic = data[8] as f64 * 0.01;

    assert!((ua - 230.5).abs() < 0.01, "Ua mismatch: {ua}");
    assert!((ub - 230.1).abs() < 0.01, "Ub mismatch: {ub}");
    assert!((uc - 229.9).abs() < 0.01, "Uc mismatch: {uc}");
    assert!((uab - 399.2).abs() < 0.01, "Uab mismatch: {uab}");
    assert!((ubc - 398.7).abs() < 0.01, "Ubc mismatch: {ubc}");
    assert!((uca - 399.0).abs() < 0.01, "Uca mismatch: {uca}");

    assert!((ia - 5.25).abs() < 0.001, "Ia mismatch: {ia}");
    assert!((ib - 5.20).abs() < 0.001, "Ib mismatch: {ib}");
    assert!((ic - 5.30).abs() < 0.001, "Ic mismatch: {ic}");
}

#[test]
fn test_sil_read_active_reactive_power_pf_and_frequency() {
    let telemetry = Adl400Telemetry {
        p_total: 6950.0,
        pa: 2310.0,
        pb: 2320.0,
        pc: 2320.0,
        q_total: 1380.0,
        qa: 460.0,
        qb: 460.0,
        qc: 460.0,
        pf_total: 0.985,
        frequency: 50.02,
        ..Default::default()
    };

    let mut meter = Adl400Simulator::new(1, telemetry);

    // Read registers 0x0042 to 0x004E (13 registers: Active Power, Reactive Power, PF, Frequency)
    let req = ModbusRequest {
        tab_id: "sil_tab".into(),
        slave_id: 1,
        function_code: 3,
        start_address: 0x0042,
        quantity: 13,
        write_value: String::new(),
    };

    let enc = encode(&req).expect("Encode request");
    let resp = meter.handle_raw_frame(&enc.frame).expect("Meter response");
    let reply = decode(&req, &enc, &resp);

    let data = reply.data.expect("Registers data");
    assert_eq!(data.len(), 13);

    let p_tot = data[0] as f64;
    let pa = data[1] as f64;
    let q_tot = data[4] as f64;
    let pf_tot = data[8] as f64 * 0.001;
    let freq = data[12] as f64 * 0.01;

    assert_eq!(p_tot, 6950.0);
    assert_eq!(pa, 2310.0);
    assert_eq!(q_tot, 1380.0);
    assert!((pf_tot - 0.985).abs() < 0.0001);
    assert!((freq - 50.02).abs() < 0.001);
}

#[test]
fn test_sil_read_32bit_energy_registers() {
    let telemetry = Adl400Telemetry {
        total_active_energy_kwh: 98765.43,
        forward_active_energy_kwh: 98700.00,
        reverse_active_energy_kwh: 65.43,
        ..Default::default()
    };

    let mut meter = Adl400Simulator::new(1, telemetry);

    // Read 0x0000 (Total Active Energy - 2 registers)
    let req = ModbusRequest {
        tab_id: "sil_tab".into(),
        slave_id: 1,
        function_code: 3,
        start_address: REG_TOTAL_ACTIVE_ENERGY_HI,
        quantity: 2,
        write_value: String::new(),
    };

    let enc = encode(&req).unwrap();
    let resp = meter.handle_raw_frame(&enc.frame).unwrap();
    let reply = decode(&req, &enc, &resp);

    let data = reply.data.unwrap();
    assert_eq!(data.len(), 2);

    let hi = data[0] as u32;
    let lo = data[1] as u32;
    let total_e_raw = (hi << 16) | lo;
    let total_e_kwh = total_e_raw as f64 * 0.01;

    assert!((total_e_kwh - 98765.43).abs() < 0.01, "Total energy mismatch: {total_e_kwh}");
}

#[test]
fn test_sil_write_device_parameters_fc16() {
    let mut meter = Adl400Simulator::new(1, Adl400Telemetry::default());

    // Write CT ratio (100) and PT ratio (1) starting at 0x0062 using FC 16 (Write Multiple Registers)
    let req = ModbusRequest {
        tab_id: "sil_tab".into(),
        slave_id: 1,
        function_code: 16,
        start_address: REG_CT_RATIO,
        quantity: 2,
        write_value: "100, 1".into(),
    };

    let enc = encode(&req).unwrap();
    let resp = meter.handle_raw_frame(&enc.frame).unwrap();
    let reply = decode(&req, &enc, &resp);

    assert!(reply.error.is_none());

    // Verify registers updated on the meter
    assert_eq!(meter.registers.get(&REG_CT_RATIO).copied(), Some(100));
    assert_eq!(meter.registers.get(&REG_PT_RATIO).copied(), Some(1));
}

#[test]
fn test_sil_exception_handling() {
    let mut meter = Adl400Simulator::new(1, Adl400Telemetry::default());

    // Request invalid function code (0x07) from ModbusRequest encoder
    let req = ModbusRequest {
        tab_id: "sil_tab".into(),
        slave_id: 1,
        function_code: 7,
        start_address: 0,
        quantity: 1,
        write_value: String::new(),
    };

    assert!(encode(&req).is_err(), "Invalid function code should fail encoding");

    // Inject raw invalid FC frame to meter — compute correct CRC so frame passes validation
    let body = vec![0x01u8, 0x07, 0x00, 0x01, 0x00, 0x01];
    let crc = crc16(&body);
    let mut raw_invalid_fc = body;
    raw_invalid_fc.push((crc & 0xFF) as u8);
    raw_invalid_fc.push((crc >> 8) as u8);

    let resp = meter.handle_raw_frame(&raw_invalid_fc).expect("Exception frame");
    assert_eq!(resp[1], 0x87, "Exception response must have high bit set on FC (0x07 | 0x80)");
    assert_eq!(resp[2], 0x01, "Exception code 0x01 (Illegal Function)");
}

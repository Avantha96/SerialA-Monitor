//! Acrel ADL400 3-Phase Energy Meter Modbus RTU Protocol & Simulator for SIL / HIL Testing.
//!
//! Models the full electrical parameter register map and RTU communication
//! specification as defined in the Acrel ADL400 user manual.

use crate::modbus::crc16;
use std::collections::HashMap;

// ── Register Addresses (Hex) ────────────────────────────────────────────────
pub const REG_TOTAL_ACTIVE_ENERGY_HI: u16 = 0x0000;
pub const REG_TOTAL_ACTIVE_ENERGY_LO: u16 = 0x0001; // 4-byte, 0.01 kWh
pub const REG_FORWARD_ACTIVE_ENERGY_HI: u16 = 0x000A;
pub const REG_FORWARD_ACTIVE_ENERGY_LO: u16 = 0x000B; // 4-byte, 0.01 kWh
pub const REG_REVERSE_ACTIVE_ENERGY_HI: u16 = 0x000C;
pub const REG_REVERSE_ACTIVE_ENERGY_LO: u16 = 0x000D; // 4-byte, 0.01 kWh
pub const REG_TOTAL_REACTIVE_ENERGY_HI: u16 = 0x001E;
pub const REG_TOTAL_REACTIVE_ENERGY_LO: u16 = 0x001F; // 4-byte, 0.01 kvarh

pub const REG_VOLTAGE_A: u16 = 0x0039; // 0.1 V (e.g. 2300 -> 230.0 V)
pub const REG_VOLTAGE_B: u16 = 0x003A; // 0.1 V
pub const REG_VOLTAGE_C: u16 = 0x003B; // 0.1 V
pub const REG_VOLTAGE_AB: u16 = 0x003C; // 0.1 V (e.g. 3985 -> 398.5 V)
pub const REG_VOLTAGE_BC: u16 = 0x003D; // 0.1 V
pub const REG_VOLTAGE_CA: u16 = 0x003E; // 0.1 V

pub const REG_CURRENT_A: u16 = 0x003F; // 0.01 A (e.g. 1025 -> 10.25 A)
pub const REG_CURRENT_B: u16 = 0x0040; // 0.01 A
pub const REG_CURRENT_C: u16 = 0x0041; // 0.01 A

pub const REG_ACTIVE_POWER_TOTAL: u16 = 0x0042; // 1 W or 0.001 kW (e.g. 7052 -> 7052 W)
pub const REG_ACTIVE_POWER_A: u16 = 0x0043;
pub const REG_ACTIVE_POWER_B: u16 = 0x0044;
pub const REG_ACTIVE_POWER_C: u16 = 0x0045;

pub const REG_REACTIVE_POWER_TOTAL: u16 = 0x0046; // 1 var (e.g. 1410 -> 1410 var)
pub const REG_REACTIVE_POWER_A: u16 = 0x0047;
pub const REG_REACTIVE_POWER_B: u16 = 0x0048;
pub const REG_REACTIVE_POWER_C: u16 = 0x0049;

pub const REG_POWER_FACTOR_TOTAL: u16 = 0x004A; // 0.001 (e.g. 980 -> 0.980)
pub const REG_POWER_FACTOR_A: u16 = 0x004B;
pub const REG_POWER_FACTOR_B: u16 = 0x004C;
pub const REG_POWER_FACTOR_C: u16 = 0x004D;

pub const REG_FREQUENCY: u16 = 0x004E; // 0.01 Hz (e.g. 5000 -> 50.00 Hz)

pub const REG_SLAVE_ADDR: u16 = 0x0060; // 1-247
pub const REG_BAUD_RATE: u16 = 0x0061; // 0: 1200, 1: 2400, 2: 4800, 3: 9600, 4: 19200, 5: 38400
pub const REG_CT_RATIO: u16 = 0x0062; // 1-9999
pub const REG_PT_RATIO: u16 = 0x0063; // 1-9999

/// Represents measured 3-phase electrical telemetry from an ADL400 meter.
#[derive(Debug, Clone, PartialEq)]
pub struct Adl400Telemetry {
    // Voltages (Volts)
    pub ua: f64,
    pub ub: f64,
    pub uc: f64,
    pub uab: f64,
    pub ubc: f64,
    pub uca: f64,

    // Currents (Amperes)
    pub ia: f64,
    pub ib: f64,
    pub ic: f64,

    // Active Power (Watts)
    pub p_total: f64,
    pub pa: f64,
    pub pb: f64,
    pub pc: f64,

    // Reactive Power (Vars)
    pub q_total: f64,
    pub qa: f64,
    pub qb: f64,
    pub qc: f64,

    // Power Factors
    pub pf_total: f64,
    pub pfa: f64,
    pub pfb: f64,
    pub pfc: f64,

    // Frequency (Hz)
    pub frequency: f64,

    // Active & Reactive Energy (kWh / kvarh)
    pub total_active_energy_kwh: f64,
    pub forward_active_energy_kwh: f64,
    pub reverse_active_energy_kwh: f64,
    pub total_reactive_energy_kvarh: f64,
}

impl Default for Adl400Telemetry {
    fn default() -> Self {
        Self {
            ua: 230.1,
            ub: 229.8,
            uc: 230.4,
            uab: 398.5,
            ubc: 399.1,
            uca: 398.8,
            ia: 10.25,
            ib: 10.18,
            ic: 10.30,
            p_total: 7052.0,
            pa: 2350.0,
            pb: 2340.0,
            pc: 2362.0,
            q_total: 1410.0,
            qa: 470.0,
            qb: 468.0,
            qc: 472.0,
            pf_total: 0.980,
            pfa: 0.980,
            pfb: 0.980,
            pfc: 0.980,
            frequency: 50.00,
            total_active_energy_kwh: 12345.67,
            forward_active_energy_kwh: 12300.50,
            reverse_active_energy_kwh: 45.17,
            total_reactive_energy_kvarh: 2450.80,
        }
    }
}

/// Simulated Acrel ADL400 device for SIL (Software-in-the-Loop) and HIL testing.
pub struct Adl400Simulator {
    pub slave_id: u8,
    pub registers: HashMap<u16, u16>,
    pub telemetry: Adl400Telemetry,
    pub rx_count: u64,
    pub tx_count: u64,
    pub error_count: u64,
}

impl Adl400Simulator {
    pub fn new(slave_id: u8, telemetry: Adl400Telemetry) -> Self {
        let mut sim = Self {
            slave_id,
            registers: HashMap::new(),
            telemetry,
            rx_count: 0,
            tx_count: 0,
            error_count: 0,
        };
        sim.sync_registers_from_telemetry();

        // Default Config Registers
        sim.registers.insert(REG_SLAVE_ADDR, slave_id as u16);
        sim.registers.insert(REG_BAUD_RATE, 3); // 9600
        sim.registers.insert(REG_CT_RATIO, 1);
        sim.registers.insert(REG_PT_RATIO, 1);

        sim
    }

    pub fn sync_registers_from_telemetry(&mut self) {
        let t = &self.telemetry;

        // Energy (32-bit: 2 registers each, 0.01 scale)
        let total_e = (t.total_active_energy_kwh * 100.0).round() as u32;
        self.registers.insert(REG_TOTAL_ACTIVE_ENERGY_HI, (total_e >> 16) as u16);
        self.registers.insert(REG_TOTAL_ACTIVE_ENERGY_LO, (total_e & 0xFFFF) as u16);

        let fwd_e = (t.forward_active_energy_kwh * 100.0).round() as u32;
        self.registers.insert(REG_FORWARD_ACTIVE_ENERGY_HI, (fwd_e >> 16) as u16);
        self.registers.insert(REG_FORWARD_ACTIVE_ENERGY_LO, (fwd_e & 0xFFFF) as u16);

        let rev_e = (t.reverse_active_energy_kwh * 100.0).round() as u32;
        self.registers.insert(REG_REVERSE_ACTIVE_ENERGY_HI, (rev_e >> 16) as u16);
        self.registers.insert(REG_REVERSE_ACTIVE_ENERGY_LO, (rev_e & 0xFFFF) as u16);

        let react_e = (t.total_reactive_energy_kvarh * 100.0).round() as u32;
        self.registers.insert(REG_TOTAL_REACTIVE_ENERGY_HI, (react_e >> 16) as u16);
        self.registers.insert(REG_TOTAL_REACTIVE_ENERGY_LO, (react_e & 0xFFFF) as u16);

        // Voltages (0.1 V scale)
        self.registers.insert(REG_VOLTAGE_A, (t.ua * 10.0).round() as u16);
        self.registers.insert(REG_VOLTAGE_B, (t.ub * 10.0).round() as u16);
        self.registers.insert(REG_VOLTAGE_C, (t.uc * 10.0).round() as u16);
        self.registers.insert(REG_VOLTAGE_AB, (t.uab * 10.0).round() as u16);
        self.registers.insert(REG_VOLTAGE_BC, (t.ubc * 10.0).round() as u16);
        self.registers.insert(REG_VOLTAGE_CA, (t.uca * 10.0).round() as u16);

        // Currents (0.01 A scale)
        self.registers.insert(REG_CURRENT_A, (t.ia * 100.0).round() as u16);
        self.registers.insert(REG_CURRENT_B, (t.ib * 100.0).round() as u16);
        self.registers.insert(REG_CURRENT_C, (t.ic * 100.0).round() as u16);

        // Active Power (1 W scale)
        self.registers.insert(REG_ACTIVE_POWER_TOTAL, t.p_total.round() as i16 as u16);
        self.registers.insert(REG_ACTIVE_POWER_A, t.pa.round() as i16 as u16);
        self.registers.insert(REG_ACTIVE_POWER_B, t.pb.round() as i16 as u16);
        self.registers.insert(REG_ACTIVE_POWER_C, t.pc.round() as i16 as u16);

        // Reactive Power (1 var scale)
        self.registers.insert(REG_REACTIVE_POWER_TOTAL, t.q_total.round() as i16 as u16);
        self.registers.insert(REG_REACTIVE_POWER_A, t.qa.round() as i16 as u16);
        self.registers.insert(REG_REACTIVE_POWER_B, t.qb.round() as i16 as u16);
        self.registers.insert(REG_REACTIVE_POWER_C, t.qc.round() as i16 as u16);

        // Power Factor (0.001 scale)
        self.registers.insert(REG_POWER_FACTOR_TOTAL, (t.pf_total * 1000.0).round() as u16);
        self.registers.insert(REG_POWER_FACTOR_A, (t.pfa * 1000.0).round() as u16);
        self.registers.insert(REG_POWER_FACTOR_B, (t.pfb * 1000.0).round() as u16);
        self.registers.insert(REG_POWER_FACTOR_C, (t.pfc * 1000.0).round() as u16);

        // Frequency (0.01 Hz scale)
        self.registers.insert(REG_FREQUENCY, (t.frequency * 100.0).round() as u16);
    }

    /// Process a raw incoming Modbus RTU byte frame and generate the exact response frame.
    pub fn handle_raw_frame(&mut self, request_frame: &[u8]) -> Option<Vec<u8>> {
        self.rx_count += 1;

        if request_frame.len() < 4 {
            self.error_count += 1;
            return None;
        }

        // Check CRC
        let body_len = request_frame.len() - 2;
        let expected_crc = crc16(&request_frame[..body_len]);
        let actual_crc = u16::from(request_frame[body_len]) | (u16::from(request_frame[body_len + 1]) << 8);

        if expected_crc != actual_crc {
            self.error_count += 1;
            return None;
        }

        let slave = request_frame[0];
        if slave != self.slave_id && slave != 0 {
            // Not for this meter
            return None;
        }

        let fc = request_frame[1];

        let make_exception = |code: u8| -> Vec<u8> {
            let mut f = vec![slave, fc | 0x80, code];
            let crc = crc16(&f);
            f.push((crc & 0xff) as u8);
            f.push((crc >> 8) as u8);
            f
        };

        match fc {
            // 03H: Read Holding Registers
            0x03 => {
                if request_frame.len() < 8 {
                    self.error_count += 1;
                    return Some(make_exception(0x03)); // Illegal Data Value
                }
                let start_addr = (u16::from(request_frame[2]) << 8) | u16::from(request_frame[3]);
                let quantity = (u16::from(request_frame[4]) << 8) | u16::from(request_frame[5]);

                if quantity == 0 || quantity > 125 {
                    self.error_count += 1;
                    return Some(make_exception(0x03)); // Illegal Data Value
                }

                let byte_count = (quantity * 2) as u8;
                let mut resp = vec![self.slave_id, 0x03, byte_count];

                for i in 0..quantity {
                    let addr = start_addr + i;
                    let val = self.registers.get(&addr).copied().unwrap_or(0);
                    resp.push((val >> 8) as u8);
                    resp.push((val & 0xFF) as u8);
                }

                let crc = crc16(&resp);
                resp.push((crc & 0xff) as u8);
                resp.push((crc >> 8) as u8);
                self.tx_count += 1;
                Some(resp)
            }

            // 06H: Write Single Register
            0x06 => {
                if request_frame.len() < 8 {
                    self.error_count += 1;
                    return Some(make_exception(0x03));
                }
                let addr = (u16::from(request_frame[2]) << 8) | u16::from(request_frame[3]);
                let val = (u16::from(request_frame[4]) << 8) | u16::from(request_frame[5]);

                self.registers.insert(addr, val);

                // Echo request back as response
                let mut resp = request_frame[..6].to_vec();
                let crc = crc16(&resp);
                resp.push((crc & 0xff) as u8);
                resp.push((crc >> 8) as u8);
                self.tx_count += 1;
                Some(resp)
            }

            // 10H (16): Write Multiple Registers
            0x10 => {
                if request_frame.len() < 9 {
                    self.error_count += 1;
                    return Some(make_exception(0x03));
                }
                let start_addr = (u16::from(request_frame[2]) << 8) | u16::from(request_frame[3]);
                let quantity = (u16::from(request_frame[4]) << 8) | u16::from(request_frame[5]);
                let byte_count = request_frame[6] as usize;

                if request_frame.len() < 7 + byte_count + 2 {
                    self.error_count += 1;
                    return Some(make_exception(0x03));
                }

                for i in 0..quantity {
                    let off = 7 + (i as usize * 2);
                    let val = (u16::from(request_frame[off]) << 8) | u16::from(request_frame[off + 1]);
                    self.registers.insert(start_addr + i, val);
                }

                // Response is: slave, 0x10, addr_hi, addr_lo, qty_hi, qty_lo, crc
                let mut resp = vec![
                    self.slave_id,
                    0x10,
                    (start_addr >> 8) as u8,
                    (start_addr & 0xFF) as u8,
                    (quantity >> 8) as u8,
                    (quantity & 0xFF) as u8,
                ];
                let crc = crc16(&resp);
                resp.push((crc & 0xff) as u8);
                resp.push((crc >> 8) as u8);
                self.tx_count += 1;
                Some(resp)
            }

            // Unsupported function code
            _ => {
                self.error_count += 1;
                Some(make_exception(0x01)) // Illegal Function
            }
        }
    }
}

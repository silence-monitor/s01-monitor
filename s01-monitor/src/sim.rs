use std::sync::Mutex;

use web_time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::decode;
use crate::snapshot::{lock, Frame, Snapshot};
use crate::source::Stepper;

const PACK_ID: &str = "2713969177";
const BMS_STATUS: [u8; 8] = [0x45, 0x82, 0x01, 0x00, 0x04, 0x0A, 0x00, 0x00];

/// Drifting sample from the dashboard mock. Values stay in engineering units,
/// which is finer than the 0.1 V wire step, so the chart moves the way the
/// mock does. Flag bytes are the captured `0x184` frame.
pub struct Simulation {
    voltage: f64,
    current: f64,
    soc: f64,
    temp_a: f64,
    temp_b: f64,
    temp_c: f64,
    cells: [f64; 14],
    speed: f64,
    range_km: f64,
    rpm: f64,
    motor: f64,
    inverter: f64,
    counter: u8,
    temp_low: f64,
    temp_high: f64,
    soc_fine: f64,
    remain: f64,
    vsoc: f64,
    soh: f64,
    max_discharge: f64,
    max_charge: f64,
    since_reset: u32,
    charged: u32,
    discharged: u32,
    regenerated: u32,
    power_bar: f64,
    drive_flags: u8,
    outside: f64,
    odometer: u32,
    throttle: f64,
    footbrake: bool,
    since_ignition: u32,
    controller: u8,
    temp_third: f64,
    heatsink: f64,
    motor_ctrl: f64,
    cap_voltage: f64,
    torque_demand: f64,
    torque_actual: f64,
    hour: u32,
    minute: u32,
    second: u32,
    millis: u32,
    t_ms: u64,
    rng: u64,
    last_step: Instant,
}

impl Simulation {
    pub fn new() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos() as u64)
            .unwrap_or(0x00A1_C3E2)
            | 1;
        Self {
            voltage: 54.2,
            current: -3.6,
            soc: 82.0,
            temp_a: 31.2,
            temp_b: 32.0,
            temp_c: 30.4,
            cells: [
                3920.0, 3905.0, 3898.0, 3884.0, 3876.0, 3872.0, 3868.0, 3864.0, 3870.0, 3880.0,
                3892.0, 3900.0, 3910.0, 3875.0,
            ],
            speed: 36.0,
            range_km: 48.0,
            rpm: 2140.0,
            motor: 46.2,
            inverter: 41.0,
            counter: 12,
            temp_low: 30.0,
            temp_high: 33.0,
            soc_fine: 820.0,
            remain: 21400.0,
            vsoc: 80.0,
            soh: 97.0,
            max_discharge: 80.0,
            max_charge: 12.0,
            since_reset: 18420,
            charged: 120_000,
            discharged: 80_000,
            regenerated: 4200,
            power_bar: 55.0,
            drive_flags: 0x21,
            outside: 14.0,
            odometer: 4218,
            throttle: 1200.0,
            footbrake: false,
            since_ignition: 1400,
            controller: 5,
            temp_third: 38.4,
            heatsink: 44.0,
            motor_ctrl: 48.1,
            cap_voltage: 54.1,
            torque_demand: 0.4,
            torque_actual: 0.2,
            hour: 14,
            minute: 32,
            second: 8,
            millis: 0,
            t_ms: 0,
            rng: seed,
            last_step: Instant::now(),
        }
    }

    fn step(&mut self) {
        self.voltage += (self.unit() - 0.48) * 0.04;
        self.current = (self.current + (self.unit() - 0.5) * 0.15).min(-0.4);
        self.soc = (self.soc + 0.01).min(100.0);
        self.temp_a += (self.unit() - 0.45) * 0.05;
        self.temp_b += (self.unit() - 0.45) * 0.05;
        self.temp_c += (self.unit() - 0.45) * 0.05;
        self.speed = (self.speed + (self.unit() - 0.5) * 0.8).max(0.0);
        self.range_km = (self.range_km - 0.01).max(0.0);
        self.rpm = (self.rpm + (self.unit() - 0.5) * 30.0).max(0.0);
        self.motor += (self.unit() - 0.45) * 0.08;
        self.inverter += (self.unit() - 0.45) * 0.06;
        self.counter = self.counter.wrapping_add(1);
        self.temp_low = nudge(self.temp_low, 0.2, 0.0, 80.0, self.unit());
        self.temp_high = nudge(self.temp_high, 0.2, 0.0, 80.0, self.unit());
        self.soc_fine = nudge(self.soc_fine, 1.0, 0.0, 1000.0, self.unit());
        self.remain = nudge(self.remain, 4.0, 0.0, 30000.0, self.unit());
        self.vsoc = nudge(self.vsoc, 0.4, 0.0, 100.0, self.unit());
        self.max_discharge = nudge(self.max_discharge, 0.4, 0.0, 120.0, self.unit());
        self.max_charge = nudge(self.max_charge, 0.2, 0.0, 40.0, self.unit());
        self.since_reset = self.since_reset.saturating_add(1);
        self.charged = self.charged.saturating_add(2);
        self.power_bar = nudge(self.power_bar, 2.0, 0.0, 100.0, self.unit());
        self.temp_third = nudge(self.temp_third, 0.08, 0.0, 90.0, self.unit());
        self.since_ignition = self.since_ignition.saturating_add(1);
        self.heatsink = nudge(self.heatsink, 0.2, 0.0, 90.0, self.unit());
        self.motor_ctrl = nudge(self.motor_ctrl, 0.08, 0.0, 90.0, self.unit());
        self.cap_voltage = nudge(self.cap_voltage, 0.04, 0.0, 80.0, self.unit());
        self.torque_demand = nudge(self.torque_demand, 0.08, -20.0, 20.0, self.unit());
        self.torque_actual = nudge(self.torque_actual, 0.08, -20.0, 20.0, self.unit());
        self.outside = nudge(self.outside, 0.1, -20.0, 50.0, self.unit());
        self.throttle = nudge(self.throttle, 40.0, 0.0, 4700.0, self.unit());
        for index in 0..self.cells.len() {
            let drift = (self.unit() - 0.5) * 1.4;
            self.cells[index] += drift;
            if index == 7 {
                self.cells[index] -= 0.2;
            }
        }
        self.t_ms = self.t_ms.saturating_add(250);
        self.millis += 250;
        while self.millis >= 1000 {
            self.millis -= 1000;
            self.second += 1;
            if self.second >= 60 {
                self.second = 0;
                self.minute += 1;
            }
            if self.minute >= 60 {
                self.minute = 0;
                self.hour = (self.hour + 1) % 24;
            }
        }
    }

    fn publish(&self, snapshot: &Mutex<Snapshot>) {
        let mut snap = lock(snapshot);
        snap.voltage = Some(self.voltage as f32);
        snap.current = Some(self.current as f32);
        snap.current_summary = Some((self.current * 10.0).round() as f32 / 10.0);
        snap.soc = Some(self.soc as f32);
        snap.temp_a = Some(self.temp_a as f32);
        snap.temp_b = Some(self.temp_b as f32);
        snap.temp_c = Some(self.temp_c as f32);
        snap.speed = Some(self.speed as f32);
        snap.range_km = Some(self.range_km as f32);
        snap.rpm = Some(self.rpm as f32);
        snap.motor = Some(self.motor as f32);
        snap.inverter = Some(self.inverter as f32);
        snap.counter = Some(self.counter);
        snap.temp_low = Some(self.temp_low.round() as i8);
        snap.temp_high = Some(self.temp_high.round() as i8);
        snap.soc_fine = Some(self.soc_fine.round() as u16);
        snap.remain_mah = Some(self.remain.round() as u16);
        snap.capacity_ah = Some((self.remain / 1000.0) as f32);
        snap.vsoc = Some(self.vsoc.round() as u8);
        snap.soh = Some(self.soh.round() as u8);
        snap.max_discharge = Some(self.max_discharge.round() as u16);
        snap.max_charge = Some(self.max_charge.round() as i16);
        snap.since_reset = Some(self.since_reset);
        snap.charged_ws = Some(self.charged);
        snap.discharged_ws = Some(self.discharged);
        snap.regenerated_ws = Some(self.regenerated);
        snap.power_bar = Some(self.power_bar.round() as u8);
        snap.drive_flags = Some(self.drive_flags);
        snap.outside = Some(self.outside.round() as i8);
        snap.odometer = Some(self.odometer);
        snap.throttle = Some(self.throttle as f32);
        snap.footbrake = Some(self.footbrake);
        snap.since_ignition = Some(self.since_ignition);
        snap.controller = Some(self.controller);
        snap.mark_char = Some("Clear".into());
        snap.mark_stan = Some("STAN".into());
        snap.temp_third = Some(self.temp_third as f32);
        snap.heatsink = Some(self.heatsink.round() as f32);
        snap.motor_ctrl = Some(self.motor_ctrl as f32);
        snap.cap_voltage = Some(self.cap_voltage as f32);
        // The controller reports draw as positive, the opposite of the pack current.
        snap.battery_current = Some(-self.current as f32);
        snap.torque_demand = Some(self.torque_demand as f32);
        snap.torque_actual = Some(self.torque_actual as f32);
        for (slot, cell) in snap.cells.iter_mut().zip(self.cells) {
            *slot = Some(cell as f32);
        }
        snap.pack_id = Some(PACK_ID.to_string());
        snap.clock = Some(format!(
            "{:02}:{:02}:{:02}",
            self.hour, self.minute, self.second
        ));
        decode::apply(&mut snap, &Frame::new(0x184, &BMS_STATUS));
        decode::apply(&mut snap, &Frame::new(0x18E, &[0; 8]));
        decode::apply(&mut snap, &Frame::new(0x191, &[0; 8]));
        decode::apply(&mut snap, &Frame::new(0x294, &[0x00, 0x52, 0x08, 0x40]));
        decode::apply(&mut snap, &Frame::new(0x401, &[0x00, 0x00, 0, 0, 0x13]));
        decode::apply(&mut snap, &Frame::new(0x28A, &[0, 0, 0, 0, 0x08]));
        decode::apply(&mut snap, &Frame::new(0x293, &[0, 0, 0, 0, 0, 0, 0x40, 0x04]));
        snap.throttle = Some(self.throttle as f32);
        snap.push_sample(self.t_ms);
    }

    fn unit(&mut self) -> f64 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng = x;
        (x >> 11) as f64 / (1u64 << 53) as f64
    }
}

fn nudge(value: f64, amount: f64, low: f64, high: f64, unit: f64) -> f64 {
    (value + (unit - 0.5) * amount).clamp(low, high)
}

impl Stepper for Simulation {
    /// Fills the first stretch of the charts straight away.
    fn begin(&mut self, snapshot: &Mutex<Snapshot>) -> Result<(), String> {
        for _ in 0..24 {
            self.step();
            self.publish(snapshot);
        }
        self.last_step = Instant::now();
        Ok(())
    }

    fn tick(&mut self, snapshot: &Mutex<Snapshot>) -> Result<(), String> {
        if self.last_step.elapsed() >= Duration::from_millis(250) {
            self.last_step = Instant::now();
            self.step();
            self.publish(snapshot);
        }
        Ok(())
    }

    fn interval(&self) -> Duration {
        Duration::from_millis(50)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_stays_at_or_below_the_charge_floor() {
        let mut sim = Simulation::new();
        for _ in 0..200 {
            sim.step();
            assert!(sim.current <= -0.4);
        }
    }
}

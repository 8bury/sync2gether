//! Relógio monotônico filtrado e correções proporcionais com histerese.
use crate::protocol::Snapshot;
use std::collections::VecDeque;

#[derive(Default, Debug)]
pub struct Clock {
    pub offset_ms: Option<f64>,
    pub rtt_ms: f64,
    pub uncertainty_ms: f64,
    samples: VecDeque<(f64, f64, f64)>,
    last_sample: f64,
    identity: bool,
}
impl Clock {
    pub fn host() -> Self {
        Self {
            offset_ms: Some(0.0),
            identity: true,
            ..Default::default()
        }
    }
    /// t1/t4 são locais; t2/t3 são recepção/envio do anfitrião.
    pub fn sample(&mut self, t1: f64, t2: f64, t3: f64, t4: f64) {
        if [t1, t2, t3, t4].iter().any(|t| !t.is_finite())
            || t4 < t1
            || t3 < t2
            || t4 < self.last_sample
        {
            return;
        }
        let rtt = (t4 - t1) - (t3 - t2);
        if !(0.0..=3000.0).contains(&rtt) {
            return;
        }
        let offset = ((t2 - t1) + (t3 - t4)) / 2.0;
        self.samples.push_back((rtt, offset, t4));
        while self.samples.len() > 16 || self.samples.front().is_some_and(|s| t4 - s.2 > 10_000.0) {
            self.samples.pop_front();
        }
        let mut best: Vec<_> = self.samples.iter().copied().collect();
        best.sort_by(|a, b| a.0.total_cmp(&b.0));
        best.truncate(5);
        let mut offsets: Vec<_> = best.iter().map(|s| s.1).collect();
        offsets.sort_by(f64::total_cmp);
        let middle = offsets[offsets.len() / 2];
        self.offset_ms = Some(
            self.offset_ms
                .map_or(middle, |old| old + (middle - old) * 0.2),
        );
        self.rtt_ms = best[0].0;
        self.uncertainty_ms = self.rtt_ms / 2.0
            + best
                .iter()
                .map(|s| (s.1 - middle).abs())
                .fold(0.0_f64, f64::max);
        self.last_sample = t4;
    }
    pub fn ready(&self, local_ms: f64) -> bool {
        self.identity || (self.samples.len() >= 3 && local_ms - self.last_sample <= 1500.0)
    }
    pub fn host_now(&self, local_ms: f64) -> Option<f64> {
        self.ready(local_ms)
            .then(|| local_ms + self.offset_ms.unwrap_or(0.0))
    }
    pub fn position(&self, state: &Snapshot, local_ms: f64) -> Option<f64> {
        self.host_now(local_ms).map(|now| state.position_at(now))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Correction {
    None,
    Speed(f64),
    Seek(f64),
}
#[derive(Default)]
pub struct Controller {
    filtered: Option<f64>,
    active: bool,
    large_since: Option<f64>,
    last_seek: Option<f64>,
    quiet_until: f64,
}
impl Controller {
    pub fn reset(&mut self, now: f64) {
        *self = Self {
            quiet_until: now + 500.0,
            ..Default::default()
        };
    }
    pub fn update(&mut self, actual: f64, target: f64, paused: bool, now: f64) -> Correction {
        if now < self.quiet_until || !actual.is_finite() || !target.is_finite() {
            return Correction::None;
        }
        let error = target - actual;
        let filtered = self
            .filtered
            .map_or(error, |old| old + (error - old) * 0.25);
        self.filtered = Some(filtered);
        let large = if paused {
            error.abs() > 0.1
        } else {
            filtered.abs() > 0.6
        };
        if large {
            let since = *self.large_since.get_or_insert(now);
            let persistence = if paused { 200.0 } else { 500.0 };
            if now - since >= persistence && self.last_seek.is_none_or(|last| now - last >= 2000.0)
            {
                self.last_seek = Some(now);
                self.quiet_until = now + 500.0;
                self.filtered = None;
                self.active = false;
                self.large_since = None;
                return Correction::Seek(target);
            }
        } else {
            self.large_since = None;
        }
        if paused {
            self.active = false;
            return Correction::None;
        }
        if self.active && filtered.abs() < 0.035 {
            self.active = false;
        } else if !self.active && filtered.abs() > 0.08 {
            self.active = true;
        }
        if self.active {
            Correction::Speed((1.0 + filtered * 0.05).clamp(0.98, 1.02))
        } else {
            Correction::None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removes_server_processing_time_and_requires_warmup() {
        let mut c = Clock::default();
        // 100 ms de rede e 200 ms de processamento remoto.
        for n in 0..3 {
            let t = n as f64 * 500.0;
            c.sample(t, t + 4050.0, t + 4250.0, t + 300.0);
        }
        assert_eq!(c.offset_ms, Some(4000.0));
        assert_eq!(c.rtt_ms, 100.0);
        assert!(c.ready(1300.0));
        assert!(!c.ready(3000.0));
        c.sample(2000.0, 0.0, -1.0, 2100.0);
        assert_eq!(c.offset_ms, Some(4000.0));
    }
    #[test]
    fn filters_out_high_delay_offset_outliers() {
        let mut c = Clock::default();
        for n in 0..8 {
            let t = n as f64 * 100.0;
            c.sample(t, t + 1005.0, t + 1005.0, t + 10.0);
        }
        c.sample(1000.0, 1700.0, 1700.0, 2000.0);
        assert!((c.offset_ms.unwrap() - 1000.0).abs() < 1.0);
    }
    #[test]
    fn proportional_speed_has_hysteresis_and_large_errors_need_persistence() {
        let mut c = Controller::default();
        assert_eq!(c.update(10.0, 10.05, false, 0.0), Correction::None);
        for n in 1..10 {
            c.update(10.0, 10.3, false, n as f64 * 50.0);
        }
        let Correction::Speed(speed) = c.update(10.0, 10.3, false, 500.0) else {
            panic!()
        };
        assert!((1.0..=1.02).contains(&speed));
        let mut c = Controller::default();
        assert!(matches!(
            c.update(10.0, 12.0, false, 0.0),
            Correction::Speed(_)
        ));
        assert_eq!(c.update(10.0, 12.0, false, 500.0), Correction::Seek(12.0));
        assert_eq!(c.update(10.0, 12.0, false, 550.0), Correction::None);
    }
    #[test]
    fn brief_spikes_do_not_seek_and_reset_gives_player_time_to_settle() {
        let mut c = Controller::default();
        c.update(0.0, 1.0, false, 0.0);
        for n in 1..20 {
            assert!(!matches!(
                c.update(0.0, 0.0, false, n as f64 * 50.0),
                Correction::Seek(_)
            ));
        }
        c.reset(1000.0);
        assert_eq!(c.update(0.0, 10.0, true, 1100.0), Correction::None);
    }
}

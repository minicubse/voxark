use cpu_time::ProcessTime;
use serde_json::json;
use std::time::{Duration, Instant};

pub struct Metrics {
    started: Instant,
    cpu: Option<ProcessTime>,
    ticks: Vec<Duration>,
    streaming: Vec<Duration>,
}
fn p(sorted: &[Duration], percent: usize) -> f64 {
    sorted[(sorted.len() * percent).div_ceil(100).saturating_sub(1)].as_secs_f64() * 1000.
}
impl Metrics {
    pub fn new() -> Self {
        Self {
            started: Instant::now(),
            cpu: ProcessTime::try_now().ok(),
            ticks: vec![],
            streaming: vec![],
        }
    }
    pub fn record(&mut self, tick: Duration, streaming: Duration) {
        self.ticks.push(tick);
        self.streaming.push(streaming);
    }
    pub fn len(&self) -> usize {
        self.ticks.len()
    }
    pub fn report(
        &mut self,
        tick: u64,
        resident: usize,
        peers: usize,
        traffic: primal_net::TrafficStats,
    ) {
        if self.ticks.is_empty() {
            return;
        }
        self.ticks.sort_unstable();
        self.streaming.sort_unstable();
        let elapsed = self.started.elapsed().as_secs_f64();
        println!(
            "{}",
            json!({"event":"metrics","tick":tick,"samples":self.len(),"period_s":elapsed,"actual_hz":self.len() as f64/elapsed,
        "tick_p50_ms":p(&self.ticks,50),"tick_p95_ms":p(&self.ticks,95),"tick_p99_ms":p(&self.ticks,99),
        "stream_p95_ms":p(&self.streaming,95),"resident":resident,"players":peers,
        "cpu_ms":self.cpu.and_then(|v|v.try_elapsed().ok()).map(|d|d.as_secs_f64()*1000.),
        "sent_bytes":traffic.sent_bytes,"received_bytes":traffic.received_bytes,"sent_messages":traffic.sent_messages,"received_messages":traffic.received_messages})
        );
        self.started = Instant::now();
        self.cpu = ProcessTime::try_now().ok();
        self.ticks.clear();
        self.streaming.clear();
    }
}

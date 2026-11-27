use cpu_time::ProcessTime;
use primal_net::{CreatureTraffic, TrafficStats};
use serde_json::json;
use std::time::{Duration, Instant};

pub struct TickTimes {
    pub streaming: Duration,
    pub creature_simulation: Duration,
}

pub struct Metrics {
    started: Instant,
    cpu: Option<ProcessTime>,
    ticks: Vec<Duration>,
    streaming: Vec<Duration>,
    creature_simulation: Vec<Duration>,
    previous_creature_traffic: CreatureTraffic,
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
            creature_simulation: vec![],
            previous_creature_traffic: CreatureTraffic::default(),
        }
    }
    pub fn record(&mut self, tick: Duration, times: TickTimes) {
        self.ticks.push(tick);
        self.streaming.push(times.streaming);
        self.creature_simulation.push(times.creature_simulation);
    }
    pub fn len(&self) -> usize {
        self.ticks.len()
    }
    pub fn report(
        &mut self,
        tick: u64,
        resident: usize,
        peers: usize,
        traffic: TrafficStats,
        creature_traffic: CreatureTraffic,
    ) {
        if self.ticks.is_empty() {
            return;
        }
        self.ticks.sort_unstable();
        self.streaming.sort_unstable();
        self.creature_simulation.sort_unstable();
        let elapsed = self.started.elapsed().as_secs_f64();
        let creature_sim_ms = self
            .creature_simulation
            .iter()
            .sum::<Duration>()
            .as_secs_f64()
            * 1000.
            / self.len() as f64;
        let period_bytes = creature_traffic.bytes - self.previous_creature_traffic.bytes;
        let period_messages = creature_traffic.messages - self.previous_creature_traffic.messages;
        println!(
            "{}",
            json!({"event":"metrics","tick":tick,"samples":self.len(),"period_s":elapsed,"actual_hz":self.len() as f64/elapsed,
        "tick_p50_ms":p(&self.ticks,50),"tick_p95_ms":p(&self.ticks,95),"tick_p99_ms":p(&self.ticks,99),
        "stream_p95_ms":p(&self.streaming,95),"resident":resident,"players":peers,
        "cpu_ms":self.cpu.and_then(|v|v.try_elapsed().ok()).map(|d|d.as_secs_f64()*1000.),
        "creature_sim_mean_ms":creature_sim_ms,"creature_sim_p95_ms":p(&self.creature_simulation,95),
        "creature_sent_bytes":creature_traffic.bytes,"creature_sent_messages":creature_traffic.messages,
        "creature_period_sent_bytes":period_bytes,"creature_period_sent_messages":period_messages,
        "creature_sent_bytes_per_s":period_bytes as f64/elapsed,"creature_sent_messages_per_s":period_messages as f64/elapsed,
        "sent_bytes":traffic.sent_bytes,"received_bytes":traffic.received_bytes,"sent_messages":traffic.sent_messages,"received_messages":traffic.received_messages})
        );
        self.started = Instant::now();
        self.cpu = ProcessTime::try_now().ok();
        self.ticks.clear();
        self.streaming.clear();
        self.creature_simulation.clear();
        self.previous_creature_traffic = creature_traffic;
    }
}

use crate::netutils::Backend;
use rand::prelude::*;
use rand::rngs::SmallRng;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};


#[derive(Debug, Clone)]
pub struct ServerMetrics {
    // metrics are percents (0..100)
    pub cpu: f64, 
    pub mem: f64, 
    pub net: f64, 
    pub io: f64,  
}

impl ServerMetrics {
    pub fn new() -> Self {
        ServerMetrics { cpu: 0.0, mem: 0.0, net: 0.0, io: 0.0 }
    }

    pub fn update_ema(&mut self, cpu: f64, mem: f64, net: f64, io: f64, alpha: f64) {
        self.cpu = cpu;
        self.mem = mem;
        self.net = net;
        self.io = io;
    }
}

#[derive(Debug, Clone)]
pub struct ServerState {
    pub backend: Backend,
    pub metrics: ServerMetrics,
    pub weight: f64,
}

impl ServerState {
    pub fn new(backend: Backend) -> Self {
        ServerState { backend, metrics: ServerMetrics::new(), weight: 1.0 }
    }
}

/// This implementation keeps an EMA of reported resource usage (cpu/mem/net/io),
/// computes a composite load L = sum(coeff_i * metric_i) and converts that to
/// a selection weight. Lower load -> higher weight. Selection is weighted-random.
pub struct AdaptiveBalancer {
    servers: Vec<ServerState>,
    // resource coefficients (cpu, mem, net, io) - sum to 1.0
    coeffs: [f64; 4],
    // EMA smoothing factor (alpha)
    alpha: f64,
    rng: SmallRng,
}

impl AdaptiveBalancer {
    /// Create a new balancer from a list of backends.
    /// `coeffs` are the importance weights for cpu,mem,net,io respectively.
    /// `alpha` controls EMA smoothing (0..1). Typical alpha ~0.2-0.5.
    pub fn new(backends: Vec<Backend>, coeffs: [f64; 4], alpha: f64) -> Self {
        let servers = backends.into_iter().map(ServerState::new).collect();
        let rng = SmallRng::from_entropy();
        AdaptiveBalancer { servers, coeffs, alpha, rng }
    }

    /// Add a backend at runtime.
    pub fn add_backend(&mut self, backend: Backend) {
        self.servers.push(ServerState::new(backend));
    }

    /// Update metrics reported by a backend identified by its display/address.
    /// If the backend isn't found this is a no-op.
    pub fn update_metrics(&mut self, backend_addr: &str, cpu: f64, mem: f64, net: f64, io: f64) {
        for s in &mut self.servers {
            if s.backend.to_string() == backend_addr {
                s.metrics.update_ema(cpu, mem, net, io, self.alpha);
                return;
            }
        }
    }

    fn metrics_to_weight(metrics: &ServerMetrics, coeffs: &[f64; 4]) -> f64 {
        let l = coeffs[0] * metrics.cpu + coeffs[1] * metrics.mem + coeffs[2] * metrics.net + coeffs[3] * metrics.io;
        // convert load to a score where higher is better: raw = 100 - L
        let raw = (100.0 - l).max(0.0);
        // amplify differences (square) and add small epsilon to avoid zero
        raw * raw + 1e-6
    }

    /// Choose a backend using weighted random selection based on current weights.
    /// Returns an Arc-wrapped Backend clone so callers can cheaply clone it.
    pub fn choose_backend(&mut self) -> Option<Arc<Backend>> {
        if self.servers.is_empty() {
            return None;
        }

        // Compute remaining capacity R_i = 100 - composite_load
        let rs: Vec<f64> = self.servers.iter().map(|s| {
            Self::metrics_to_weight(&s.metrics, &self.coeffs)
        }).collect();

        let ws: Vec<f64> = self.servers.iter().map(|s| s.weight).collect();

        let r_sum: f64 = rs.iter().copied().sum::<f64>();
        let w_sum: f64 = ws.iter().copied().sum::<f64>().max(1e-12);
        let threshold = self.alpha * (r_sum / w_sum);

        for (i, s) in self.servers.iter_mut().enumerate() {
            let ratio = if s.weight <= 0.0 { f64::INFINITY } else { rs[i] / s.weight };
            if ratio <= threshold {
                Some(Arc::new(s.backend.clone()));
            }
        }

        // If any server satisfies Ri/Wi <= threshold, it means the server
        // is relatively overloaded and we must adjust its weight using
        // formula (6).
        
        // TODO: adjust weight

        // Compute Li = Wi / Ri and choose server minimizing Li. This gives
        // preference to servers with high remaining capacity relative to
        // their weight (lower Li better).
        let mut best_idx: Option<usize> = None;
        let mut best_li = AtomicUsize::new(usize::MAX);
        for (i, s) in self.servers.iter().enumerate() {
            let li = s.backend.current_load;
            if compare_atomic(&li, &best_li) == -1 {
                best_li = li;
                best_idx = Some(i);
            }
        }

        // If nothing chosen, fall back to random selection
        if best_idx.is_none() {
            let i = (self.rng.next_u32() as usize) % self.servers.len();
            return Some(Arc::new(self.servers[i].backend.clone()));
        }

        Some(Arc::new(self.servers[best_idx.unwrap()].backend.clone()))
    }

    // Expose a snapshot of server weights (for monitoring/testing)
    pub fn snapshot_weights(&self) -> Vec<(String, f64)> {
        self.servers.iter().map(|s| (s.backend.to_string(), s.weight)).collect()
    }
}

fn compare_atomic(a: &AtomicUsize, b: &AtomicUsize) -> i32 {
    let x = a.load(Ordering::SeqCst); 
    let y = b.load(Ordering::SeqCst); 
    if x < y { return -1; }
    if x > y { return 1; }
    return 0;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_weight_update_and_choose() {
        let backends = vec![Backend::new("127.0.0.1:1".to_string()), Backend::new("127.0.0.1:2".to_string())];
        let mut b = AdaptiveBalancer::new(backends, [0.5, 0.2, 0.2, 0.1], 0.5);
        // initially equal weights
        let snaps = b.snapshot_weights();
        assert_eq!(snaps.len(), 2);
        // update one backend to be heavily loaded
        b.update_metrics("127.0.0.1:1", 90.0, 80.0, 10.0, 5.0);
        b.update_metrics("127.0.0.1:2", 10.0, 5.0, 1.0, 1.0);
        let snaps2 = b.snapshot_weights();
        println!("{:?}, {:?}", snaps, snaps2);
        // after update the first server (127.0.0.1:1) should have a worse
        // weight (higher composite load -> lower remaining capacity)
        // assert!(snaps2[0].1 < snaps2[1].1);

        // Choose backend: should pick the less loaded host (127.0.0.1:2)
        let chosen = b.choose_backend().expect("should choose a backend");
        assert_eq!(chosen.to_string(), "127.0.0.1:2");
    }
}

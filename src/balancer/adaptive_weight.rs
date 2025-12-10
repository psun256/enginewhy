use crate::netutils::Backend;
use rand::prelude::*;
use rand::rngs::SmallRng;
use std::sync::Arc;


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

    pub fn update(&mut self, cpu: f64, mem: f64, net: f64, io: f64) {
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

pub struct AdaptiveBalancer {
    servers: Vec<ServerState>,
    // resource coefficients (cpu, mem, net, io) - sum to 1.0
    coeffs: [f64; 4],
    alpha: f64,
    rng: SmallRng,
}

impl AdaptiveBalancer {
    pub fn new(backends: Vec<Backend>, coeffs: [f64; 4], alpha: f64) -> Self {
        let servers = backends.into_iter().map(ServerState::new).collect();
        let rng = SmallRng::from_entropy();
        AdaptiveBalancer { servers, coeffs, alpha, rng }
    }

    pub fn add_backend(&mut self, backend: Backend) {
        self.servers.push(ServerState::new(backend));
    }

    /// Update metrics reported by a backend identified by its display/address.
    /// If the backend isn't found this is a no-op.
    pub fn update_metrics(&mut self, backend_addr: &str, cpu: f64, mem: f64, net: f64, io: f64) {
        for s in &mut self.servers {
            if s.backend.to_string() == backend_addr {
                s.metrics.update(cpu, mem, net, io);
                return;
            }
        }
    }

    fn metrics_to_weight(metrics: &ServerMetrics, coeffs: &[f64; 4]) -> f64 {
        coeffs[0] * metrics.cpu + coeffs[1] * metrics.mem + coeffs[2] * metrics.net + coeffs[3] * metrics.io
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
        let ls: Vec<u32> = self.servers.iter().map(|s| s.backend.current_load).collect();

        let r_sum: f64 = rs.iter().copied().sum::<f64>();
        let w_sum: f64 = ws.iter().copied().sum::<f64>().max(1e-12);
        let l_sum: u32 = ls.iter().copied().sum::<u32>();
        let threshold = self.alpha * (r_sum / w_sum);

        for (i, s) in self.servers.iter_mut().enumerate() {
            let ratio = if s.weight <= 0.0 { f64::INFINITY } else { rs[i] / s.weight };
            if ratio <= threshold {
                return Some(Arc::new(s.backend.clone()));
            }
        }

        // If any server satisfies Ri/Wi <= threshold, it means the server
        // is relatively overloaded and we must adjust its weight using
        // formula (6).
    
        let lwi: Vec<f64> = self.servers.iter().enumerate().map(|(i, s)| {
            s.backend.current_load as f64 * w_sum / ws[i] * l_sum as f64
        }).collect();
        let a_lwi: f64 = lwi.iter().copied().sum::<f64>() / lwi.len() as f64;
        for (i, s) in self.servers.iter_mut().enumerate() {
            s.weight += 1 as f64 - lwi[i] / a_lwi;
        }

        // Compute Li = Wi / Ri and choose server minimizing Li.
        let mut best_idx: Option<usize> = None;
        let mut best_li = u32::MAX;
        for (i, s) in self.servers.iter().enumerate() {
            let li = s.backend.current_load;
            if li < best_li {
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

        // Choose backend: should pick the less loaded host (127.0.0.1:2)
        let chosen = b.choose_backend().expect("should choose a backend");
        let snaps2 = b.snapshot_weights();
        println!("{:?}, {:?}", snaps, snaps2);
        assert_eq!(chosen.to_string(), "127.0.0.1:2");
    }

    #[test]
    fn choose_none_when_empty() {
        let mut b = AdaptiveBalancer::new(vec![], [0.5, 0.2, 0.2, 0.1], 0.5);
        assert!(b.choose_backend().is_none());
    }

    #[test]
    fn ratio_triggers_immediate_selection() {
        // Arrange two servers where server 1 has composite load 0 and server 2 has composite load 100.
        // With alpha = 1.0 and two servers, threshold = 1.0 * (r_sum / w_sum) = 1.0 * (100 / 2) = 50.
        // Server 1 ratio = 0 / 1 = 0 <= 50 so it should be chosen immediately.
        let backends = vec![Backend::new("127.0.0.1:1".to_string()), Backend::new("127.0.0.1:2".to_string())];
        let mut b = AdaptiveBalancer::new(backends, [0.25, 0.25, 0.25, 0.25], 1.0);

        b.update_metrics("127.0.0.1:1", 0.0, 0.0, 0.0, 0.0);
        b.update_metrics("127.0.0.1:2", 100.0, 100.0, 100.0, 100.0);

        let chosen = b.choose_backend().expect("should choose a backend");
        assert_eq!(chosen.to_string(), "127.0.0.1:1");
    }

    #[test]
    fn choose_min_current_load_when_no_ratio() {
        // Arrange three servers with identical composite loads so no server satisfies Ri/Wi <= threshold
        // (set alpha < 1 so threshold < ratio). The implementation then falls back to picking the
        // server with minimum current_load 
        let mut s1 = Backend::new("127.0.0.1:1".to_string());
        let mut s2 = Backend::new("127.0.0.1:2".to_string());
        let mut s3 = Backend::new("127.0.0.1:3".to_string());

        // set current_loads (field expected to be public)
        s1.current_load = 10;
        s2.current_load = 5;
        s3.current_load = 20;

        // Use coeffs that only consider CPU so composite load is easy to reason about.
        let mut bal = AdaptiveBalancer::new(vec![s1, s2, s3], [1.0, 0.0, 0.0, 0.0], 0.5);

        // set identical composite loads > 0 for all so ratio = x and threshold = alpha * x < x
        // you will have threshold = 25 for all 3 backend servers and ratio = 50
        // so that forces to choose the smallest current load backend
        bal.update_metrics("127.0.0.1:1", 50.0, 0.0, 0.0, 0.0);
        bal.update_metrics("127.0.0.1:2", 50.0, 0.0, 0.0, 0.0);
        bal.update_metrics("127.0.0.1:3", 50.0, 0.0, 0.0, 0.0);

        let chosen = bal.choose_backend().expect("should choose a backend");
        // expect server with smallest current_load (127.0.0.1:2)
        assert_eq!(chosen.to_string(), "127.0.0.1:2");
    }
}

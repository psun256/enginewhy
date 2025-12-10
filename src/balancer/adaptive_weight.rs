use crate::backend::{Backend, BackendPool, ServerMetrics};
use crate::balancer::{Balancer, ConnectionInfo};
use rand::prelude::*;
use rand::rngs::SmallRng;
use std::fmt::Debug;
use std::fs::Metadata;
use std::sync::{Arc, RwLock};

#[derive(Debug)]
struct AdaptiveNode {
    backend: Arc<Backend>,
    weight: f64,
}

#[derive(Debug)]
pub struct AdaptiveWeightBalancer {
    pool: Vec<AdaptiveNode>,
    coefficients: [f64; 4],
    alpha: f64,
    rng: SmallRng,
}

impl AdaptiveWeightBalancer {
    pub fn new(pool: BackendPool, coefficients: [f64; 4], alpha: f64) -> Self {
        let nodes = pool
            .backends
            .iter()
            .map(|b| AdaptiveNode {
                backend: b.clone(),
                weight: 1f64,
            })
            .collect();

        AdaptiveWeightBalancer {
            pool: nodes,
            coefficients,
            alpha,
            rng: SmallRng::from_rng(&mut rand::rng()),
        }
    }

    pub fn metrics_to_weight(&self, metrics: &ServerMetrics) -> f64 {
        self.coefficients[0] * metrics.cpu
            + self.coefficients[1] * metrics.mem
            + self.coefficients[2] * metrics.net
            + self.coefficients[3] * metrics.io
    }
}

impl Balancer for AdaptiveWeightBalancer {
    fn choose_backend(&mut self, ctx: ConnectionInfo) -> Option<Arc<Backend>> {
        if self.pool.is_empty() {
            return None;
        }

        // Compute remaining capacity R_i = 100 - composite_load
        let mut r_sum = 0.0;
        let mut w_sum = 0.0;
        let mut l_sum = 0;

        for node in &self.pool {
            if let Ok(health) = node.backend.metrics.read() {
                r_sum += self.metrics_to_weight(&health);
            }
            w_sum += node.weight;
            l_sum += node
                .backend
                .active_connections
                .load(std::sync::atomic::Ordering::Relaxed);
        }

        let safe_w_sum = w_sum.max(1e-12);
        let threshold = self.alpha * (r_sum / safe_w_sum);
        
        for idx in 0..self.pool.len() {
            let node = &self.pool[idx];

            if node.weight <= 0.001 {
                continue;
            }

            let risk = match node.backend.metrics.read() {
                Ok(h) => self.metrics_to_weight(&h),
                Err(_) => f64::MAX,
            };

            let ratio = risk / node.weight;
            if ratio <= threshold {
                return Some(node.backend.clone());
            }
        }

        // If any server satisfies Ri/Wi <= threshold, it means the server
        // is relatively overloaded, and we must adjust its weight using
        // formula (6).
        let mut total_lwi = 0.0;
        let l_sum_f64 = l_sum as f64;

        for node in &self.pool {
            let load = node
                .backend
                .active_connections
                .load(std::sync::atomic::Ordering::Relaxed) as f64;
            let weight = node.weight.max(1e-12);
            let lwi = load * (safe_w_sum / weight) * l_sum_f64;
            total_lwi += lwi;
        }

        let avg_lwi = (total_lwi / self.pool.len() as f64).max(1e-12);

        // Compute Li = Wi / Ri and choose server minimizing Li.
        let mut best_backend: Option<Arc<Backend>> = None;
        let mut min_load = usize::MAX;

        for node in &mut self.pool {
            let load = node
                .backend
                .active_connections
                .load(std::sync::atomic::Ordering::Relaxed);
            let load_f64 = load as f64;
            let weight = node.weight.max(1e-12);

            let lwi = load_f64 * (safe_w_sum / weight) * l_sum_f64;

            let adj = 1.0 - (lwi / avg_lwi);
            node.weight += adj;

            node.weight = node.weight.clamp(0.1, 100.0);
            if load < min_load {
                min_load = load;
                best_backend = Some(node.backend.clone());
            }
        }

        match best_backend {
            Some(backend) => Some(backend),
            None => {
                let i = (self.rng.next_u32() as usize) % self.pool.len();
                Some(self.pool[i].backend.clone())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::Backend;
    use std::net::SocketAddr;

    fn backend_factory(id: &str, ip: &str, port: u16) -> Arc<Backend> {
        Arc::new(Backend::new(
            id.to_string(),
            SocketAddr::new(ip.parse().unwrap(), port),
            Arc::new(RwLock::new(ServerMetrics::default())),
        ))
    }

    fn unused_ctx() -> ConnectionInfo {
        ConnectionInfo {
            client_ip: ("0.0.0.0".parse().unwrap()),
        }
    }

    #[test]
    fn basic_weight_update_and_choose() {
        let backends = BackendPool::new(vec![
            backend_factory("server-0", "127.0.0.1", 3000),
            backend_factory("server-1", "127.0.0.1", 3001),
        ]);
        let mut b = AdaptiveWeightBalancer::new(backends.clone(), [0.5, 0.2, 0.2, 0.1], 0.5);
        // initially equal weights
        // update one backend to be heavily loaded
        {
            let mut sm0_guard = backends.backends.get(0).unwrap().metrics.write().unwrap();
            sm0_guard.update(90.0, 80.0, 10.0, 5.0);
        }
        {
            let mut sm1_guard = backends.backends.get(1).unwrap().metrics.write().unwrap();
            sm1_guard.update(10.0, 5.0, 1.0, 1.0);
        }

        // Choose backend: should pick the less loaded host server1
        let chosen = b
            .choose_backend(unused_ctx())
            .expect("should choose a backend");

        let sm0: &ServerMetrics = &backends.backends.get(0).unwrap().metrics.read().unwrap();
        let sm1: &ServerMetrics = &backends.backends.get(1).unwrap().metrics.read().unwrap();
        println!("{:?}, {:?}", sm0, sm1);
        assert_eq!(chosen.id, "server-1");
    }

    #[test]
    fn choose_none_when_empty() {
        let mut b =
            AdaptiveWeightBalancer::new(BackendPool::new(vec![]), [0.5, 0.2, 0.2, 0.1], 0.5);
        assert!(b.choose_backend(unused_ctx()).is_none());
    }

    #[test]
    fn ratio_triggers_immediate_selection() {
        // Arrange two servers where server 1 has composite load 0 and server 2 has composite load 100.
        // With alpha = 1.0 and two servers, threshold = 1.0 * (r_sum / w_sum) = 1.0 * (100 / 2) = 50.
        // Server 0 ratio = 0 / 1 = 0 <= 50 so it should be chosen immediately.
        let backends = BackendPool::new(vec![
            backend_factory("server-0", "127.0.0.1", 3000),
            backend_factory("server-1", "127.0.0.1", 3001),
        ]);
        let mut b = AdaptiveWeightBalancer::new(backends.clone(), [0.25, 0.25, 0.25, 0.25], 1.0);

        {
            let mut sm0_guard = backends.backends.get(0).unwrap().metrics.write().unwrap();
            sm0_guard.update(0.0, 0.0, 0.0, 0.0);
        }
        {
            let mut sm1_guard = backends.backends.get(1).unwrap().metrics.write().unwrap();
            sm1_guard.update(100.0, 100.0, 100.0, 100.0);
        }

        let chosen = b
            .choose_backend(unused_ctx())
            .expect("should choose a backend");
        assert_eq!(chosen.id, "server-0");
    }

    #[test]
    fn choose_min_current_load_when_no_ratio() {
        // Arrange three servers with identical composite loads so no server satisfies Ri/Wi <= threshold
        // (set alpha < 1 so threshold < ratio). The implementation then falls back to picking the
        // server with minimum current_load
        let backends = BackendPool::new(vec![
            backend_factory("server-0", "127.0.0.1", 3000),
            backend_factory("server-1", "127.0.0.1", 3001),
            backend_factory("server-2", "127.0.0.1", 3002),
        ]);

        // set current_loads (field expected to be public)

        {
            let mut sm0_guard = backends.backends.get(0).unwrap().metrics.write().unwrap();
            sm0_guard.update(10.0, 10.0, 10.0, 10.0);
        }
        {
            let mut sm1_guard = backends.backends.get(1).unwrap().metrics.write().unwrap();
            sm1_guard.update(5.0, 5.0, 5.0, 5.0);
        }
        {
            let mut sm2_guard = backends.backends.get(2).unwrap().metrics.write().unwrap();
            sm2_guard.update(20.0, 20.0, 20.0, 20.0);
        }

        // Use coeffs that only consider CPU so composite load is easy to reason about.
        let mut bal = AdaptiveWeightBalancer::new(backends.clone(), [1.0, 0.0, 0.0, 0.0], 0.5);

        // set identical composite loads > 0 for all so ratio = x and threshold = alpha * x < x
        // you will have threshold = 25 for all 3 backend servers and ratio = 50
        // so that forces to choose the smallest current load backend

        let chosen = bal
            .choose_backend(unused_ctx())
            .expect("should choose a backend");
        // expect server with smallest current_load server-1
        assert_eq!(chosen.id, "server-1");
    }
}

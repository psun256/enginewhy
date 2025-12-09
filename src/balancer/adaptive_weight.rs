use std::sync::{Arc, RwLock};
use std::fmt::Debug;
use std::fs::Metadata;
use crate::backend::{Backend, BackendPool, ServerHealth};
use crate::balancer::Balancer;
use rand::prelude::*;
use rand::rngs::SmallRng;

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
        let nodes = pool.backends
            .iter()
            .map(|b| AdaptiveNode {
                backend: b.clone(),
                weight: 0f64,
            })
            .collect();

        AdaptiveWeightBalancer {
            pool: nodes,
            coefficients,
            alpha,
            rng: SmallRng::from_rng(&mut rand::rng())
        }
    }

    pub fn metrics_to_weight(&self, metrics: &ServerHealth) -> f64 {
        self.coefficients[0] * metrics.cpu +
        self.coefficients[1] * metrics.mem +
        self.coefficients[2] * metrics.net +
        self.coefficients[3] * metrics.io
    }
}

impl Balancer for AdaptiveWeightBalancer {
    fn choose_backend(&mut self) -> Option<Arc<Backend>> {
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
            l_sum += node.backend.active_connections
                .load(std::sync::atomic::Ordering::Relaxed);
        }

        let safe_w_sum = w_sum.max(1e-12);
        let threshold = self.alpha * (r_sum / safe_w_sum);

        for idx in 0..self.pool.len() {
            let node = &self.pool[idx];

            if node.weight <= 0.001 { continue; }

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
            let load = node.backend.active_connections
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
            let load = node.backend.active_connections
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
use crate::backend::{Backend, BackendPool};
use crate::balancer::{Balancer, ConnectionInfo};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{Arc};

#[derive(Debug)]
pub struct SourceIPHash {
    pool: BackendPool,
}

impl SourceIPHash {
    pub fn new(pool: BackendPool) -> SourceIPHash {
        Self { pool }
    }
}

impl Balancer for SourceIPHash {
    fn choose_backend(&mut self, ctx: ConnectionInfo) -> Option<Arc<Backend>> {
        let client_ip = ctx.client_ip;

        let mut hasher = DefaultHasher::new();
        client_ip.hash(&mut hasher);
        let hash = hasher.finish();
        let idx = (hash as usize) % self.pool.backends.len();

        Some(self.pool.backends[idx].clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;
    use crate::backend::ServerMetrics;

    fn create_dummy_backends(count: usize) -> BackendPool {
        let mut backends = Vec::new();
        for i in 1..=count {
            backends.push(Arc::new(Backend::new(
                format!("backend {}", i),
                format!("127.0.0.1:808{}", i).parse().unwrap(),
                Arc::new(RwLock::new(ServerMetrics::default())),
            )));
        }
        BackendPool::new(backends)
    }

    #[test]
    fn test_same_ip_always_selects_same_backend() {
        let backends = create_dummy_backends(3);
        let mut balancer = SourceIPHash::new(backends);

        let client_ip: IpAddr = "192.168.1.100".parse().unwrap();

        let first_choice = balancer.choose_backend(ConnectionInfo { client_ip });
        let second_choice = balancer.choose_backend(ConnectionInfo { client_ip });

        assert!(first_choice.is_some());
        assert!(second_choice.is_some());

        let first = first_choice.unwrap();
        let second = second_choice.unwrap();

        assert_eq!(first.id, second.id);
    }

    #[test]
    fn test_different_ips_may_select_different_backends() {
        let backends = create_dummy_backends(2);
        let mut balancer = SourceIPHash::new(backends);

        let ip1: IpAddr = "192.168.1.100".parse().unwrap();
        let choice1 = balancer.choose_backend(ConnectionInfo { client_ip: ip1 });

        let ip2: IpAddr = "192.168.1.101".parse().unwrap();
        let choice2 = balancer.choose_backend(ConnectionInfo { client_ip: ip2 });

        assert!(choice1.is_some());
        assert!(choice2.is_some());
    }

    #[test]
    fn test_hash_distribution_across_backends() {
        let pool = create_dummy_backends(3);
        let backends_ref = pool.backends.clone();

        let mut balancer = SourceIPHash::new(pool);
        let mut distribution = [0, 0, 0];

        // Test 30 different IPs
        for i in 0..30 {
            let client_ip: IpAddr = format!("192.168.1.{}", 100 + i).parse().unwrap();

            if let Some(backend) = balancer.choose_backend(ConnectionInfo { client_ip }) {
                for (idx, b) in backends_ref.iter().enumerate() {
                    if backend.id == b.id && backend.address == b.address {
                        distribution[idx] += 1;
                        break;
                    }
                }
            }
        }

        assert!(distribution[0] > 0, "Backend 0 received no traffic");
        assert!(distribution[1] > 0, "Backend 1 received no traffic");
        assert!(distribution[2] > 0, "Backend 2 received no traffic");
    }
}
use crate::backend::{Backend, BackendPool, ServerHealth};
use crate::balancer::{Balancer, CURRENT_CONNECTION_INFO, ConnectionInfo};
use std::hash::{Hasher, DefaultHasher, Hash};
use std::sync::{Arc, RwLock};

#[derive(Debug)]
pub struct SourceIPHash {
    pool : BackendPool,
}

impl SourceIPHash {
    pub fn new(pool: BackendPool) -> SourceIPHash {
        Self { pool }
    }
}

impl Balancer for SourceIPHash {
    fn choose_backend(&mut self) -> Option<Arc<Backend>>{
        let client_ip = CURRENT_CONNECTION_INFO.with(|info| {
            info.borrow().as_ref().map(|c| c.client_ip.clone())
        });

        let client_ip = match client_ip {
            Some(ip) => ip,
            None => return None, // no client info available
        };

        let mut hasher = DefaultHasher::new();
        client_ip.hash(&mut hasher);
        let hash = hasher.finish();
        let idx = (hash as usize) % self.pool.backends.len();

        return Some(self.pool.backends[idx].clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_same_ip_always_selects_same_backend() {
        let backends = vec![
            Arc::new(Backend::new(
                "backend 1".into(),
                "127.0.0.1:8081".parse().unwrap(),
                Arc::new(RwLock::new(ServerHealth::default())),
            )),
            Arc::new(Backend::new(
                "backend 2".into(),
                "127.0.0.1:8082".parse().unwrap(),
                Arc::new(RwLock::new(ServerHealth::default())),
            )),
            Arc::new(Backend::new(
                "backend 3".into(),
                "127.0.0.1:8083".parse().unwrap(),
                Arc::new(RwLock::new(ServerHealth::default())),
            )),
        ];

        let mut balancer = SourceIPHash::new(BackendPool::new(backends));
        let client_ip = "192.168.1.100:54321".parse().unwrap();

        CURRENT_CONNECTION_INFO.with(|info| {
            *info.borrow_mut() = Some(ConnectionInfo { client_ip });
        });

        let first_choice = balancer.choose_backend();
        
        CURRENT_CONNECTION_INFO.with(|info| {
            *info.borrow_mut() = Some(ConnectionInfo { client_ip });
        });

        let second_choice = balancer.choose_backend();

        assert!(first_choice.is_some());
        assert!(second_choice.is_some());
        let first = first_choice.unwrap();
        let second = second_choice.unwrap();
        assert_eq!(first.id, second.id);

        // Cleanup
        CURRENT_CONNECTION_INFO.with(|info| {
            *info.borrow_mut() = None;
        });
    }

    #[test]
    fn test_different_ips_may_select_different_backends() {
        let backends = vec![
            Arc::new(Backend::new(
                "backend 1".into(),
                "127.0.0.1:8081".parse().unwrap(),
                Arc::new(RwLock::new(ServerHealth::default())),
            )),
            Arc::new(Backend::new(
                "backend 2".into(),
                "127.0.0.1:8082".parse().unwrap(),
                Arc::new(RwLock::new(ServerHealth::default())),
            )),
        ];

        let mut balancer = SourceIPHash::new(BackendPool::new(backends));

        let ip1 = "192.168.1.100:54321".parse().unwrap();
        CURRENT_CONNECTION_INFO.with(|info| {
            *info.borrow_mut() = Some(ConnectionInfo { client_ip: ip1 });
        });
        let choice1 = balancer.choose_backend();

        let ip2 = "192.168.1.101:54322".parse().unwrap();
        CURRENT_CONNECTION_INFO.with(|info| {
            *info.borrow_mut() = Some(ConnectionInfo { client_ip: ip2 });
        });
        let choice2 = balancer.choose_backend();

        assert!(choice1.is_some());
        assert!(choice2.is_some());
        // Note: choice1 and choice2 might be equal by chance, but statistically should differ

        CURRENT_CONNECTION_INFO.with(|info| {
            *info.borrow_mut() = None;
        });
    }

    #[test]
    fn test_returns_none_when_no_connection_info() {
        let backends = vec![Arc::new(Backend::new(
            "backend 1".into(),
            "127.0.0.1:8081".parse().unwrap(),
            Arc::new(RwLock::new(ServerHealth::default())),
        ))];

        let mut balancer = SourceIPHash::new(BackendPool::new(backends));

        // Don't set any connection info
        CURRENT_CONNECTION_INFO.with(|info| {
            *info.borrow_mut() = None;
        });

        let choice = balancer.choose_backend();
        assert!(choice.is_none());
    }

    #[test]
    fn test_hash_distribution_across_backends() {
        let backends = vec![
            Arc::new(Backend::new(
                "backend 1".into(),
                "127.0.0.1:8081".parse().unwrap(),
                Arc::new(RwLock::new(ServerHealth::default())),
            )),
            Arc::new(Backend::new(
                "backend 2".into(),
                "127.0.0.1:8082".parse().unwrap(),
                Arc::new(RwLock::new(ServerHealth::default())),
            )),
            Arc::new(Backend::new(
                "backend 3".into(),
                "127.0.0.1:8083".parse().unwrap(),
                Arc::new(RwLock::new(ServerHealth::default())),
            )),
        ];

        let mut balancer = SourceIPHash::new(BackendPool::new(backends.clone()));
        let mut distribution = [0, 0, 0];

        // Test 30 different IPs to see if they distribute across backends
        for i in 0..30 {
            let client_ip = format!("192.168.1.{}:54321", 100 + i).parse().unwrap();
            CURRENT_CONNECTION_INFO.with(|info| {
                *info.borrow_mut() = Some(ConnectionInfo { client_ip });
            });

            if let Some(backend) = balancer.choose_backend() {
                for (idx, b) in backends.iter().enumerate() {
                    if backend.id == b.id && backend.address == b.address {
                        distribution[idx] += 1;
                        break;
                    }
                }
            }
        }

        assert!(distribution[0] > 0);
        assert!(distribution[1] > 0);
        assert!(distribution[2] > 0);

        CURRENT_CONNECTION_INFO.with(|info| {
            *info.borrow_mut() = None;
        });
    }
}
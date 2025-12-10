use cidr::IpCidr;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, RwLock};

use crate::backend::*;
use crate::backend::health::*;
use crate::balancer::Balancer;
use crate::balancer::adaptive_weight::AdaptiveWeightBalancer;
use crate::balancer::round_robin::RoundRobinBalancer;
use crate::config::*;

pub struct RoutingTable {
    pub balancers: Vec<Box<dyn Balancer + Send>>,
    pub entries: Vec<(IpCidr, usize)>,
}

fn parse_client(s: &str) -> (IpCidr, u16) {
    // just splits "0.0.0.0/0:80" into ("0.0.0.0/0", 80)
    let (ip_part, port_part) = s.rsplit_once(':').expect("badly formatted client");

    let port: u16 = port_part.parse().expect("bad port");
    let cidr: IpCidr = ip_part.parse().expect("bad ip/mask");

    (cidr, port)
}

pub type PortListeners = HashMap<u16, RoutingTable>;

pub fn build_lb(config: AppConfig) -> (PortListeners, HashMap<IpAddr, Arc<RwLock<ServerMetrics>>>) {
    let mut healths: HashMap<IpAddr, Arc<RwLock<ServerMetrics>>> = HashMap::new();
    let mut backends: HashMap<String, Arc<Backend>> = HashMap::new();

    for backend_cfg in config.backends {
        let ip: IpAddr = backend_cfg.ip.parse().unwrap();
        let addr = SocketAddr::new(ip, backend_cfg.port);

        let health = healths
            .entry(ip)
            .or_insert_with(|| Arc::new(RwLock::new(ServerMetrics::default())))
            .clone();

        let backend = Arc::new(Backend::new(backend_cfg.id.clone(), addr, health));

        backends.insert(backend_cfg.id, backend);
    }

    let mut listeners: PortListeners = HashMap::new();

    for rule in config.rules {
        let mut target_backends = Vec::new();

        for target_name in &rule.targets {
            if let Some(members) = config.clusters.get(target_name) {
                for member_id in members {
                    if let Some(backend) = backends.get(member_id) {
                        target_backends.push(backend.clone());
                    }
                }
            } else if let Some(backend) = backends.get(target_name) {
                target_backends.push(backend.clone());
            } else {
                eprintln!("warning: target {} not found", target_name);
            }
        }

        // possible for multiple targets of the same rule to have common backends.
        target_backends.sort_by(|a, b| a.id.cmp(&b.id));
        target_backends.dedup_by(|a, b| a.id == b.id);

        if target_backends.is_empty() {
            eprintln!("warning: rule has no valid targets, skipping.");
            continue;
        }

        // for each different client port on this rule, we unfortunately need to make a new
        // Balancer, since Balancer is not thread safe, requires &mut self for the backend
        // selection.
        // a good enough compromise to make a new one for each port, avoids using Mutex, at the
        // cost of minor penalty to load balancing "quality" when you have several client ports.
        let mut port_groups: HashMap<u16, Vec<IpCidr>> = HashMap::new();

        for client_def in rule.clients {
            let (cidr, port) = parse_client(&client_def);
            port_groups.entry(port).or_default().push(cidr);
        }

        for (port, cidrs) in port_groups {
            let table = listeners.entry(port).or_insert_with(|| RoutingTable {
                balancers: Vec::new(),
                entries: Vec::new(),
            });

            let pool = BackendPool::new(target_backends.clone());

            let balancer: Box<dyn Balancer + Send> = match &rule.strategy {
                LoadBalancerStrategy::RoundRobin => Box::new(RoundRobinBalancer::new(pool)),
                LoadBalancerStrategy::Adaptive {
                    coefficients,
                    alpha,
                } => Box::new(AdaptiveWeightBalancer::new(pool, *coefficients, *alpha)),
            };

            let balancer_idx = table.balancers.len();
            table.balancers.push(balancer);

            for cidr in cidrs {
                table.entries.push((cidr, balancer_idx));
            }
        }
    }

    // sort to make most specific first, so that first match == longest prefix match
    for table in listeners.values_mut() {
        table
            .entries
            .sort_by(|(a, _), (b, _)| a.network_length().cmp(&b.network_length()));
    }

    (listeners, healths)
}

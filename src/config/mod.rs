// config is written as a YAML file, the path will be passed to the program.
//
// the high level structure of the config is that we
// first define the individual backends (ip + port) we are going
// to load balance around.
//
// next we define some clusters, which are really more like a short
// alias for a group of backends.
//
// next we define the rules. these are written as a list of
// "ip/subnet:port" for the clients, and then a list of clusters
// for which backends these are balanced around. and of course
// specify which algorithm to use.
pub mod loader;

use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize)]
pub struct AppConfig {
    pub backends: Vec<BackendConfig>,
    #[serde(default)]
    pub clusters: HashMap<String, Vec<String>>,
    pub rules: Vec<RuleConfig>,
}

#[derive(Debug, Deserialize)]
pub struct BackendConfig {
    pub id: String,
    pub ip: String,
    pub port: u16,
}

#[derive(Debug, Deserialize)]
pub struct RuleConfig {
    pub clients: Vec<String>,
    pub targets: Vec<String>,
    pub strategy: LoadBalancerStrategy,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum LoadBalancerStrategy {
    RoundRobin,
    Adaptive { coefficients: [f64; 4], alpha: f64 },
}

// Physical server health statistics, used for certain load balancing algorithms
#[derive(Debug, Default)]
pub struct ServerMetrics {
    pub cpu: f64,
    pub mem: f64,
    pub net: f64,
    pub io: f64,
}

impl ServerMetrics {
    pub fn update(&mut self, cpu: f64, mem: f64, net: f64, io: f64) {
        self.cpu = cpu;
        self.mem = mem;
        self.net = net;
        self.io = io;
    }
}

use sysinfo::{CpuRefreshKind, RefreshKind, System};
use sysinfo::{Networks};
use sysinfo::{Disks};
use std::thread;
use std::time::Duration;
use std::net::{TcpStream};
use std::env;
use std::collections::HashMap;
use std::io::Write;
use serde_json::Value;
use rperf3::{Client, Config, Protocol};


// Default server addresses
const DEFAULT_REMOTE_IP: &str = "192.67.67.67";
const DEFAULT_LOCAL_IP: &str = "127.0.0.1";
const PORT: u16 = 8080;
const IPERF_PORT: u16 = 5001;

fn get_io_usage_percentage() -> Result<f64, String> {
    let mut sys = Disks::new_with_refreshed_list();
    
    // Refresh disk information
    sys.refresh(true);
    
    // Get first disk (usually main disk)
    if let Some(disk) = sys.list().first() {
        let initial_read = disk.usage().total_read_bytes;
        let initial_write = disk.usage().total_written_bytes;
        
        thread::sleep(Duration::from_secs(1));   // 1s
        
        sys.refresh(true);
        let disk = sys.list().first().ok_or("Disk disappeared")?;
        
        let new_read = disk.usage().total_read_bytes;
        let new_write = disk.usage().total_written_bytes;
        
        // Calculate Bps
        let read_per_sec = (new_read - initial_read) as f64; 
        let write_per_sec = (new_write - initial_write) as f64;
        
        // Get disk type to estimate max speed (these are rough estimates)
        let max_speed = match disk.kind() {
            sysinfo::DiskKind::SSD => 500_000_000.0, // 500 MBps
            sysinfo::DiskKind::HDD => 200_000_000.0, // 200 MBps
            _ => 300_000_000.0, // Default
        };
        
        let io_percentage = f64::min(100.0, ((read_per_sec + write_per_sec) / max_speed) * 100.0);
        Ok(io_percentage) 
    } else {
        Err("No disks found".to_string())
    }
}

async fn measure_iperf_bandwidth(server_ip: &str, port: u16) -> Result<f64, Box<dyn std::error::Error>> {
    // Configure the test (use the provided port)
    let config = Config::client(server_ip.to_string(), port)
        .with_duration(Duration::from_secs(10));

    // Run the test
    let client = Client::new(config)?;
    client.run().await?;

    // Get results
    let measurements = client.get_measurements();
    let bandwidth_bps = measurements.total_bits_per_second();
    println!("iperf3 reported max bandwidth: {:.2} Mbps", bandwidth_bps / 1_000_000.0);
    
    Ok(bandwidth_bps)
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    // Determine server IP from CLI: `--localhost` -> local, otherwise remote
    let args: Vec<String> = env::args().collect();
    let server_ip = if args.iter().any(|a| a == "--localhost") {
        DEFAULT_LOCAL_IP.to_string()
    } else {
        DEFAULT_REMOTE_IP.to_string()
    };

    let mut stream = TcpStream::connect(format!("{}:{}", server_ip, PORT))?;
    println!("server connected to {}:{}", server_ip, PORT);

    // Initialize the system struct
    let mut sys = System::new_with_specifics(
        RefreshKind::nothing().with_cpu(CpuRefreshKind::everything()),
    );
    let mut networks = Networks::new();
    networks.refresh(true);

    // Probe max bandwidth using iperf3
    let mut max_bps: f64 = 0.0;
    match measure_iperf_bandwidth(&server_ip, IPERF_PORT).await {
        Ok(bps) => {
            max_bps = bps;
            println!("iperf3 reported max bandwidth: {:.2} bits/sec ({:.2} Mbps)", max_bps, max_bps / 1e6);
        }   
        Err(e) => println!("iperf3 failed: {}", e),
    }

    // Wait a bit because CPU usage is based on diff.
    std::thread::sleep(sysinfo::MINIMUM_CPU_UPDATE_INTERVAL);
    loop {
        sys.refresh_all();

        sys.refresh_cpu_usage(); // Refreshing CPU usage.
        let mut cpu_usage: f64 = 0.0;
        for cpu in sys.cpus() {
            cpu_usage += cpu.cpu_usage() as f64;
        }
        cpu_usage /= sys.cpus().len() as f64;
        println!("CPU usage is {}%", cpu_usage);

        // Memory usage
        let total_mem = sys.total_memory();
        let used_mem = sys.used_memory();
        let mem_usage = total_mem as f64 / used_mem as f64;
        println!("Memory usage is {}%", mem_usage);

        // Network bandwidth usage
        let mut bandwidth: f64 = 0.0;  // Bps
        for (interface_name, network) in &networks {
            if interface_name == "wlp2s0" {
                bandwidth = network.transmitted() as f64;
                println!("[{interface_name}] transferred {:?} %", bandwidth / max_bps * 100.0);
            }
        }
        networks.refresh(true);

        // Calculate percent usage of measured max bandwidth (if available)
        let net_usage_pct: f64 = if max_bps > 0.0 {
            f64::min(100.0, (bandwidth / max_bps) * 100.0)
        } else { 0.0 };

        // IO usage
        let mut io_usage = 0.0;
        match get_io_usage_percentage() {
            Ok(percentage) => {
                io_usage = percentage;
                println!("I/O usage is {}%", percentage)
            },
            Err(e) => println!("Error: {}", e)
        }
        println!();

        let mut packet: HashMap<String, Value> = HashMap::new();
        packet.insert("cpu".to_string(), Value::from(cpu_usage));    // %
        packet.insert("mem".to_string(), Value::from(mem_usage));    // %
        packet.insert("net".to_string(), Value::from(net_usage_pct));
        packet.insert("io".to_string(), Value::from(io_usage));

        let serialized_packet = serde_json::to_string(&packet)?;
        serialized_packet.push('\n');
        let _ = stream.write(serialized_packet.as_bytes());

        thread::sleep(Duration::from_secs(10));
    }
}


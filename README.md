# nginy

EngineWhy, also known as nginy, is a simple but flexible TCP load balancer.

## Contents
- [Features](#features)
- [Getting started](#getting-started)
  - [Running as a standalone binary](#running-as-a-standalone-binary)
  - [Running as a dockerized application](#running-as-a-dockerized-application)
- [Configuration](#configuration)
- [Examples](#examples)

## Features

- Multiple different load balancing algorithms
  - Round Robin
  - IP Hashing
  - Adaptive algorithm (based on [this paper](https://www.wcse.org/WCSE_2018/W110.pdf))
- Cluster based backend grouping, and CIDR based client matching for fine-grained control.
- YAML based config for readability and simplicity.
- Automatic hot-reload of configuration without interruptions to service.

## Getting started

### Running as a standalone binary
You must have the latest stable Rust release (1.91 at the time of writing), and a valid configuration file (see [Configuration](#configuration) for details).

1. Clone the repository:
```sh
git clone https://github.com/psun256/enginewhy.git
cd enginewhy
```
2. Build the application:
```sh
cargo build --release
```
3. Run the application:
```sh
./target/release/l4lb -c path/to/config.yaml
```

### Running as a dockerized application
You may also consider running the load balancer as a dockerized application.

1. Clone the repository:
```sh
git clone https://github.com/psun256/enginewhy.git
cd enginewhy
```
2. Build the application:
```sh
docker build -t enginewhy .
```
3. Run the application:
```sh
docker run -v "path/to/config.yaml:/enginewhy/config.yaml" enginewhy
```

## Configuration
Configuration file is written in YAML.
By default, the program will look for a file named `config.yaml` in the working directory.
You can change this by specifying the path with `-c` or `--config` when running the program.

The file consists of:
- Defining the health response and iperf port (IP + port).
- A set of backends (IP + Port for each).
- A set of clusters, which act as group aliases for a set of backends.
- A list of rules, which consist of:
  - Clients: One or more CIDR + port number, used to match incoming client connection
  - Targets: One or more clusters / backend names.
  - Strategy: A load balancing algorithm to use.
    - Some algorithms have additional configuration, like the Adaptive algorithm.

Sample configuration:
```yml
healthcheck_addr: "10.0.1.10:9000"

iperf_addr: "10.0.1.10:5201"

backends:
  - id: "srv-1"
    ip: "10.0.1.11:8081"
  - id: "srv-2"
    ip: "10.0.1.12:8082"
  - id: "srv-3"
    ip: "10.0.1.13:8083"
  - id: "srv-4"
    ip: "10.0.1.14:8084"

clusters:
  main-api:
    - "srv-1"
    - "srv-2"
  priority-api:
    - "srv-3"
    - "srv-4"

rules:
  - clients:
      - "0.0.0.0/0:8080"
    targets:
      - "main-api"
    strategy:
      type: "RoundRobin"

  - clients:
      - "10.0.0.0/24:8080"
      - "10.0.0.0/24:25565"
    targets:
      - "main-api"
      - "priority-api"
    strategy:
      type: "RoundRobin"
```

An incoming client will be matched with whatever rule has the longest matching prefix on the correct port.

## Examples

You can find some examples in the `examples` directory of the project.

To run these, just run `docker compose up`.

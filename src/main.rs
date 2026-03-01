use ferrox::gateway::{GatewayConfig, GatewayError};

fn main() -> Result<(), GatewayError> {
    let config = GatewayConfig::default();

    eprintln!("Ferrox - Order Matching Engine");
    eprintln!("ferrox v{}", env!("CARGO_PKG_VERSION"));
    eprintln!("  tcp listen:  {}", config.listen_addr);
    eprintln!("  udp multicast: {}", config.multicast_addr);
    eprintln!("  ring capacity: {}", config.ring_capacity);
    eprintln!("  arena capacity: {}", config.arena_capacity);
    match &config.data_dir {
        Some(dir) => eprintln!("  data dir:    {}", dir.display()),
        None => eprintln!("  persistence: disabled"),
    }
    eprintln!();

    ferrox::gateway::run(config)
}

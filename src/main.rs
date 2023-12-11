use arti_client::config::BoolOrAuto;
use arti_client::{DataStream, StreamPrefs, TorClient, TorClientConfig};
use std::sync::Arc;
use structopt::StructOpt;
use tokio::net::TcpListener;

#[derive(Debug, StructOpt)]
#[structopt(name = "OrPipe", about = "Tor Proxy to Local")]
struct CliOptions {
    #[structopt(short, long, parse(from_str = parse_binding))]
    bindings: Vec<Bindings>,
}

#[derive(Debug, Clone)]
struct Bindings {
    onion: String,
    from_port: u16,
    local: String,
    to_port: u16,
}

fn parse_binding(s: &str) -> Bindings {
    let parts: Vec<&str> = s.split('~').collect();
    let (address1, port1) = split_address(parts[0]);
    let (address2, port2) = split_address(parts[1]);

    Bindings {
        onion: address1,
        from_port: port1,
        local: address2,
        to_port: port2,
    }
}

fn split_address(s: &str) -> (String, u16) {
    let parts: Vec<&str> = s.split(':').collect();
    (parts[0].to_string(), parts[1].parse().unwrap())
}

#[warn(unreachable_code)]
#[tokio::main]
async fn main() -> Result<(), tokio::io::Error> {
    let opt = CliOptions::from_args();

    // Use an Arc to share the bindings across tasks
    let bindings = Arc::new(opt.bindings);

    let config = TorClientConfig::default();
    let mut prefs = StreamPrefs::new();
    prefs.connect_to_onion_services(BoolOrAuto::Explicit(true));

    let mut torc = TorClient::create_bootstrapped(config).await.unwrap();
    torc.set_stream_prefs(prefs);

    // Collect all server tasks
    let mut server_tasks = Vec::new();

    for binding in bindings.iter() {
        let s_binding = Arc::new(binding);
        let torc_inner = Arc::new(torc.clone());

        let local_a = String::from(s_binding.local.as_str());
        let local_p = s_binding.to_port;
        let onion_a = String::from(s_binding.onion.as_str());
        let onion_p = s_binding.from_port;

        let server_task = tokio::task::spawn(async move {
            println!("Listen on: {}:{}", local_a, local_p);
            let listener = TcpListener::bind(format!("{}:{}", local_a, local_p)).await?;
            let onion_a1 = onion_a;

            loop {
                let onion_a2 = onion_a1.clone();
                let (socket, _) = listener.accept().await?;
                println!("Connect to: {}:{}", onion_a2, onion_p);
                let stream = torc_inner.connect((onion_a2, onion_p)).await.unwrap();

                tokio::spawn(forward_stream(socket, stream));
            }

            Result::<(), tokio::io::Error>::Ok(())
        });

        server_tasks.push(server_task);
    }

    // Await all server tasks to complete
    for server_task in server_tasks {
        let _ = server_task.await?;
    }

    Ok(())
}

async fn forward_stream(
    mut local: tokio::net::TcpStream,
    remote: DataStream,
) -> Result<(), tokio::io::Error> {
    let (mut local_read, mut local_write) = local.split();
    let (mut remote_read, mut remote_write) = remote.split();
    tokio::select! {
        _ = async {
            tokio::io::copy(&mut remote_read, &mut local_write).await?;
            Ok::<(), tokio::io::Error>(())
        } => {}
        _ = async {
            tokio::io::copy(&mut local_read, &mut remote_write).await?;
            Ok::<(), tokio::io::Error>(())
        } => {}
        else => {}
    };
    Ok(())
}

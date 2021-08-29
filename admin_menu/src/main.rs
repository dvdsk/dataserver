use std::net::{IpAddr, Ipv4Addr};

use structopt::StructOpt;
// import WorldClient comes from tarpc derive macro
use interface::rpc::{WorldClient, Codec, tarpc};

// mod menu;
// mod remote;

// use remote::Connection;

/// A basic example
#[derive(StructOpt)]
#[structopt(name = "dataserver-menu")]
struct Opt {
	/// dataserver menu port
	#[structopt(short = "p", long = "port")]
	port: u16,
}

#[tokio::main]
async fn main() {
    let opt = Opt::from_args();
    // let conn = remote::Connection::from_port(opt.port);
    // menu::command_line_interface(conn);
    
    let addr = (IpAddr::V4(Ipv4Addr::LOCALHOST), opt.port);
    let transport = tarpc::serde_transport::tcp::connect(addr, Codec::default);

    let config = crate::tarpc::client::Config::default();
    let client = WorldClient::new(config, transport.await.unwrap()).spawn();

    use tarpc::context;
    let hello = client.hello(context::current(), "david".to_owned()).await;
    dbg!(hello);

}

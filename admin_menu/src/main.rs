use structopt::StructOpt;

mod menu;
mod remote;

use remote::Connection;

/// A basic example
#[derive(StructOpt)]
#[structopt(name = "dataserver-menu")]
struct Opt {
	/// dataserver menu port
	#[structopt(short = "p", long = "port")]
	port: u16,
}

fn main() {
    let opt = Opt::from_args();
    let conn = remote::Connection::from_port(opt.port);
    menu::command_line_interface(conn);
}

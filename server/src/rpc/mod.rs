use std::net::{IpAddr, Ipv4Addr};
use futures::{future, prelude::*};
use tarpc::context;
use tarpc::server::{self, incoming::Incoming, Channel};
use tarpc::serde_transport;
use tarpc::tokio_serde::formats::Json;

use crate::data_store::Data;
use crate::database::{AlarmDatabase, PasswordDatabase, UserDatabase, UserLookup};

#[tarpc::service]
pub trait World {
    /// Returns a greeting for name.
    async fn hello(name: String) -> String;
}

#[derive(Clone)]
struct HelloServer();

#[tarpc::server]
impl World for HelloServer {
    async fn hello(self, _: context::Context, name: String) -> String {
        format!("Hello, {}!", name)
    }
}

pub async fn host(port: u16) {
    // let codec = tokio_serde::formats::Bincode::default;
    let codec = Json::default;
    let addr = (IpAddr::V4(Ipv4Addr::LOCALHOST), port);
    let listener = serde_transport::tcp::listen(addr, codec).await.unwrap();
    listener
        .filter_map(|r| future::ready(r.ok()))
        .map(server::BaseChannel::with_defaults)
        .max_channels_per_key(1, |t| t.transport().peer_addr().unwrap().ip())
        .map(|channel| {
            let server = HelloServer();
            channel.execute(server.serve())
        })
        // .bufferd_unorderd(10)
        .for_each(|_| async {})
        .await;
}

extern crate actix;
extern crate actix_web;
extern crate rand;
use self::actix::prelude::*;

use std::collections::{HashMap, HashSet};

use crate::httpserver::timeseries_interface::{DatasetId};

pub struct DataServer {
	sessions: HashMap<u16, Clientinfo>,
	subs: HashMap<DatasetId, HashSet<u16>>,
}


#[derive(Message, Clone)]
pub struct NewData {
	pub from_id: DatasetId,
	pub line: Vec<u8>,
	pub timestamp: i64,
}

impl Handler<NewData> for DataServer {
	type Result = ();

	fn handle(&mut self, msg: NewData, _: &mut Context<Self>) -> Self::Result {
		//debug!("NewData, subs: {:?}", self.subs);
		let updated_dataset_id = msg.from_id;
		//get a list of clients connected to the datasource with new data
		if let Some(subs) = self.subs.get(&updated_dataset_id){
			debug!("subs: {:?}", subs);
			for client_session_id in subs.iter() {
				//println!("sending signal");
				// foward new data message to actor that maintains the
				// websocket connection with this client.
				let client_websocket_handler = &self.sessions.get(client_session_id).unwrap().addr;
				client_websocket_handler.do_send(msg.clone()).unwrap();
			}
		}
	}
}

/// New chat session is created
#[derive(Message)]
#[rtype(u16)]
pub struct Connect {
	pub addr: Recipient<NewData>,
	pub ws_session_id: u16,
}

impl Handler<Connect> for DataServer {
	type Result = u16;

	fn handle(&mut self, msg: Connect, _: &mut Context<Self>) -> Self::Result {
		// register session with random id
		let id = msg.ws_session_id;
		self.sessions.insert(
			id,
			Clientinfo {
				addr: msg.addr,
				subs: Vec::new(),
			},
		);

		// send id back
		id
	}
}

#[derive(Message)]
pub struct Disconnect {
	pub ws_session_id: u16,
}

/// Handler for Disconnect message.
impl Handler<Disconnect> for DataServer {
	type Result = ();

	fn handle(&mut self, msg: Disconnect, _: &mut Context<Self>) {
		// remove address
		if let Some(client_info) = self.sessions.remove(&msg.ws_session_id) {
			for sub in client_info.subs {
				if let Some(subbed_clients) = self.subs.get_mut(&sub) {
					subbed_clients.remove(&msg.ws_session_id);
					trace!("removed client from: sub:{:?} ", sub);
				}
			}
		}
	}
}

/// New chat session is created
#[derive(Message)]
pub struct SubscribeToSource {
	pub ws_session_id: u16,
	pub set_id: DatasetId,
}

impl Handler<SubscribeToSource> for DataServer {
	type Result = ();

	fn handle(&mut self, msg: SubscribeToSource, _: &mut Context<Self>) -> Self::Result {
		let SubscribeToSource { ws_session_id, set_id } = msg;
		let client_info = self.sessions.get_mut(&ws_session_id).unwrap();
		client_info.subs.push(set_id);

		trace!("subscribing to source: {:?}",set_id);
		//fix when non lexical borrow checker arrives
		if let Some(subscribers) = self.subs.get_mut(&set_id) {
			subscribers.insert(ws_session_id);
			//println!("added new id to subs");
			return ();
		}

		let mut subscribers = HashSet::new();
		subscribers.insert(ws_session_id);
		self.subs.insert(set_id, subscribers);
		()
	}
}

pub struct Clientinfo {
	addr: Recipient<NewData>,
	subs: Vec<DatasetId>,
}

impl Default for DataServer {
	fn default() -> DataServer {
		let subs = HashMap::new();

		DataServer {
			sessions: HashMap::new(),
			subs: subs,
		}
	}
}

/// Make actor from `ChatServer`
impl Actor for DataServer {
	/// We are going to use simple Context, we just need ability to communicate
	/// with other actors.
	type Context = Context<Self>;
}

///////////////////////////////////

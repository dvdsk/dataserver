use self::actix::Addr;
use self::actix::*;

extern crate actix;
extern crate actix_net;
extern crate actix_web;
extern crate actix_web_httpauth;

use self::actix_web::ws;
use super::futures::Future;

use std::sync::{Arc,RwLock};
use std::collections::HashMap;


use super::timeseries_interface;
use super::websocket_data_router;
use super::WebServerData;

// store data in here, it can then be accessed using self
pub struct WsSession {
	/// unique session id
	pub session_id: u16,
	pub subscribed_fields: HashMap<timeseries_interface::DatasetId, Vec<timeseries_interface::FieldId>>,
	pub timeseries_with_access: Arc<RwLock<HashMap<timeseries_interface::DatasetId, Vec<timeseries_interface::Authorisation>>>>,
}

impl Actor for WsSession {
	type Context = ws::WebsocketContext<Self, WebServerData>;

	fn started(&mut self, ctx: &mut Self::Context) {
		// register self in chat server. `AsyncContext::wait` register
		// future within context, but context waits until this future resolves
		// before processing any other events.
		// HttpContext::state() is instance of WsChatSessionState, state is shared
		// across all routes within application

		println!("TEST");
		let addr = ctx.address();
		ctx.state()
            .websocket_addr
            .send(websocket_data_router::Connect {
                addr: addr.recipient(),
                session_id: self.session_id,
            })
            .wait();
	}

	fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
		// notify chat server
		ctx.state()
			.websocket_addr
			.do_send(websocket_data_router::Disconnect { session_id: self.session_id });
		Running::Stop
	}
}

/// send messages to server if requested by dataserver
impl Handler<websocket_data_router::NewData> for WsSession {
	type Result = ();

	fn handle(&mut self, _msg: websocket_data_router::NewData, _ctx: &mut Self::Context) {
		println!("client handler recieved signal there is new data");
		
		//ctx.binary();
	}
}

impl WsSession {
	
	fn attempt_subscribe(&mut self, args: Vec<&str>, websocket_addr: &Addr<websocket_data_router::DataServer>){
		if let Ok(set_id) = args[1].parse::<timeseries_interface::DatasetId>() {
			//check if user has access to the requested dataset
			if let Some(fields_with_access) = self.timeseries_with_access.read().unwrap().get(&set_id){
				//parse requested fields
				if let Ok(field_ids) = args[2..]
					.into_iter()
					.map(|arg| arg.parse::<timeseries_interface::FieldId>())
					.collect::<Result<Vec<timeseries_interface::FieldId>,std::num::ParseIntError>>(){
					
					let mut fields = Vec::with_capacity(args[2..].len());
					for field_id in field_ids { 
						if fields_with_access.binary_search_by(|auth| auth.as_ref().cmp(&field_id)).is_ok() { 
							fields.push(field_id);
						} else { 
							fields.truncate(0);
							break;
						}
					}
					if fields.len() > 0 {
						self.subscribed_fields.insert(set_id, fields);
						websocket_addr.do_send( websocket_data_router::SubscribeToSource {
								session_id: self.session_id,
								set_id: set_id,
						})
					} else { warn!("unautorised field requested") };
				} else { warn!("invalid field requested") };
			}
		} else { warn!("no access to dataset"); }
	}
}

/// Handler for `ws::Message`
impl StreamHandler<ws::Message, ws::ProtocolError> for WsSession {
	
	fn handle(&mut self, msg: ws::Message, ctx: &mut Self::Context) {
		// process websocket messages
		println!("WS: {:?}", msg);
		match msg {
			ws::Message::Text(text) => {
				let m = text.trim();
				if m.starts_with('/') {
					let args: Vec<&str> = m.splitn(2, ' ').collect();
					match args[0] {
						"/sub" => self.attempt_subscribe(args, &ctx.state().websocket_addr),
						"/name" => {}
						_ => ctx.text(format!("!!! unknown command: {:?}", m)),
					}
				}
			} //handle other websocket commands
			ws::Message::Ping(msg) => ctx.pong(&msg),
			ws::Message::Binary(bin) => ctx.binary(bin),
			ws::Message::Close(_) => {
				ctx.state().websocket_addr.do_send(websocket_data_router::Disconnect {session_id: self.session_id,});
				ctx.stop();
			}
			_ => (),
		}
	}
}

use self::actix::Addr;
use self::actix::*;

extern crate actix;
extern crate actix_net;
extern crate actix_web;
extern crate actix_web_httpauth;

use self::actix_web::{Binary,ws};
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
	//pub subscribed_fields: HashMap<timeseries_interface::DatasetId, Vec<SubbedField>>,
	pub subscribed_data: Vec<SubbedSet>,
	pub timeseries_with_access: Arc<RwLock<HashMap<timeseries_interface::DatasetId, Vec<timeseries_interface::Authorisation>>>>,
}

pub struct SubbedSet {
	dataset_id: timeseries_interface::DatasetId,
	field_ids: Vec<timeseries_interface::FieldId>,
	
}

#[derive(Serialize, Deserialize)]
struct Line {
    r#type: String,
    //color: String,
    name: String,
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
				println!("args: {:?}",args);
				if let Ok(field_ids) = args[2..]
					.into_iter()
					.map(|arg| arg.parse::<timeseries_interface::FieldId>())
					.collect::<Result<Vec<timeseries_interface::FieldId>,std::num::ParseIntError>>(){
					println!("field_ids: {:?}",field_ids);
					
					let mut subbed_fields = Vec::with_capacity(args[2..].len());
					for field_id in field_ids { 
						println!("field_id: {}",field_id);
						if fields_with_access.binary_search_by(|auth| auth.as_ref().cmp(&field_id)).is_ok() { 
							subbed_fields.push(field_id);
						} else { 
							warn!("unautorised field requested");
							return;
						}
					}
					self.subscribed_data.push(SubbedSet {dataset_id: set_id, field_ids: subbed_fields });
					websocket_addr.do_send( websocket_data_router::SubscribeToSource {
							session_id: self.session_id,
							set_id: set_id,
					})
				} else { warn!("invalid field requested") };
			} else { warn!("invalid dataset id"); }
		} else { warn!("no access to dataset"); }
	}

	//TODO find out what this should do 
	fn send_metadata(&mut self, data: &Arc<RwLock<timeseries_interface::Data>>) -> String{
		let data = data.read().unwrap();
		let mut client_plot_metadata = Vec::with_capacity(self.subscribed_data.len());
		
		for SubbedSet {dataset_id, field_ids} in self.subscribed_data.drain(..) {
			println!("dataset_id: {}, field_ids: {:?}",&dataset_id, &field_ids);
			let metadata = &data.sets.get(&dataset_id).unwrap().metadata;
			for field_id in field_ids {
				let field = &metadata.fields[field_id as usize];
				client_plot_metadata.push( Line {
					r#type: "scattergl".to_string(),
					name: field.name.to_owned(),
				});
			}
		}
		let json = serde_json::to_string(&client_plot_metadata).unwrap();
		println!("json: {}",&json);
		json
	}
	
	//TODO find out what this should do 
	fn send_encoding(&mut self, data: &Arc<RwLock<timeseries_interface::Data>>){
		//let data = data.read().unwrap();
		
		//for (dataset_id, fields) in self.subscribed_fields.iter() {
			//let metadata = &data.sets.get(&dataset_id).unwrap().metadata;
		//}
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
					let args: Vec<&str> = m.split_whitespace().collect();
					println!("args: {:?}",args);
					match args[0] {
						"/sub" => self.attempt_subscribe(args, &ctx.state().websocket_addr),
						"/meta" => ctx.text(self.send_metadata(&ctx.state().data)),
						"/plotData" => ctx.binary(Binary::from(self.send_metadata(&ctx.state().data))),
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

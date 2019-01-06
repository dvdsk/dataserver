use self::actix::Addr;
use self::actix::*;

extern crate actix;
extern crate actix_net;
extern crate actix_web;
extern crate chrono;
extern crate smallvec;
extern crate bincode;
extern crate serde_derive;

extern crate byteorder;

use self::byteorder::{LittleEndian, WriteBytesExt};

use self::actix_web::{Binary,ws};
use self::chrono::{Utc, Duration};
use super::futures::Future;

use std::sync::{Arc,RwLock};
use std::collections::HashMap;

use super::timeseries_interface;
use super::websocket_data_router;
use super::WebServerData;
use super::super::config;

// store data in here, it can then be accessed using self
pub struct WsSession {
	/// unique session id
	pub http_session_id: u16,
	pub ws_session_id: u16,
	//pub subscribed_fields: HashMap<timeseries_interface::DatasetId, Vec<SubbedField>>,
	pub compression_enabled: bool,
	pub selected_data: HashMap<timeseries_interface::DatasetId, Vec<timeseries_interface::FieldId>>,
	pub timeseries_with_access: Arc<RwLock<HashMap<timeseries_interface::DatasetId, Vec<timeseries_interface::Authorisation>>>>,
}

#[derive(Serialize, Deserialize)]
struct Line {
    r#type: String,
    mode: String,
    //color: String,
    name: String,
}

#[derive(Serialize, Deserialize)]
struct IdInfo {
		dataset_id: timeseries_interface::DatasetId,
		field_id: timeseries_interface::FieldId,
}

#[derive(Serialize, Deserialize, Default)]
struct MetaForPlot {
		id_info: Vec<IdInfo>,
    lines: Vec<Line>,
}

impl Actor for WsSession {
	type Context = ws::WebsocketContext<Self, WebServerData>;

	fn started(&mut self, ctx: &mut Self::Context) {
		// register self in chat server. `AsyncContext::wait` register
		// future within context, but context waits until this future resolves
		// before processing any other events.
		// HttpContext::state() is instance of WsChatSessionState, state is shared
		// across all routes within application

		let addr = ctx.address();
		ctx.state()
            .websocket_addr
            .send(websocket_data_router::Connect {
                addr: addr.recipient(),
                ws_session_id: self.ws_session_id,
            })
            .wait().unwrap();
	}

	fn stopping(&mut self, ctx: &mut Self::Context) -> Running {
		// notify chat server
		ctx.state()
			.websocket_addr
			.do_send(websocket_data_router::Disconnect { ws_session_id: self.ws_session_id });
		Running::Stop
	}
}

/// send messages to server if requested by dataserver
impl Handler<websocket_data_router::NewData> for WsSession {
	type Result = ();

	fn handle(&mut self, msg: websocket_data_router::NewData, ctx: &mut Self::Context) {
		trace!("client handler recieved signal there is new data");
		//recode data for this user
		let websocket_data_router::NewData{from_id, line, timestamp} = msg;

		let fields = self.selected_data.get(&from_id).unwrap();
		let data = ctx.state().data.read().unwrap();
		//let mut data = ctx.state().data.write().unwrap();
		let dataset = data.sets.get(&from_id).unwrap();
		info!("creating line for fields: {:?}, for set: {}",fields, from_id);
		let line = if self.compression_enabled {
			dataset.get_update(line, timestamp, fields, from_id)
		} else {
			dataset.get_update_uncompressed(line, timestamp, fields, from_id)
		};
		std::mem::drop(data);
		//send update
		debug!("sending update");
		ctx.binary(Binary::from(line));
	}
}

fn divide_ceil(x: u64, y: u64) -> u64{
	(x + y - 1) / y
}

impl WsSession {
	
	fn select_data(&mut self, args: Vec<&str>, compressed: bool){
		if args.len() < 3 {return }
		if let Ok(set_id) = args[1].parse::<timeseries_interface::DatasetId>() {
			//check if user has access to the requested dataset
			if let Some(fields_with_access) = self.timeseries_with_access.read().unwrap().get(&set_id){
				//parse requested fields
				if let Ok(field_ids) = args[2..]
					.into_iter()
					.map(|arg| arg.parse::<timeseries_interface::FieldId>())
					.collect::<Result<Vec<timeseries_interface::FieldId>,std::num::ParseIntError>>(){
					
					let mut subbed_fields = Vec::with_capacity(field_ids.len());
					for field_id in field_ids { 
						//prevent users requesting a field twice (this leads to an overflow later)
						if subbed_fields.contains(&field_id) {
							warn!("field was requested twice, ignoring duplicate");
						} else if fields_with_access.binary_search_by(|auth| auth.as_ref().cmp(&field_id)).is_ok() {
							subbed_fields.push(field_id);
						} else { 
							warn!("unautorised field requested");
							return;
						}
					}
					self.selected_data.insert(set_id, subbed_fields);
					self.compression_enabled = compressed;
				} else { warn!("invalid field requested") };
			} else { warn!("invalid dataset id"); }
		} else { warn!("no access to dataset"); }
	}

	fn subscribe(&mut self, websocket_addr: &Addr<websocket_data_router::DataServer>){
		for set_id in self.selected_data.keys(){
			websocket_addr.do_send( websocket_data_router::SubscribeToSource {
				ws_session_id: self.ws_session_id,
				set_id: *set_id,
			});
		}
	}

	//TODO implement unsubscribe command
	fn unsubscribe(&mut self, _websocket_addr: &Addr<websocket_data_router::DataServer>){
		unimplemented!();
	}

	fn send_metadata(&mut self, data: &Arc<RwLock<timeseries_interface::Data>>) -> String{
		let data = data.read().unwrap();
		let mut client_plot_metadata: MetaForPlot = Default::default();
		
		for (dataset_id, field_ids) in &self.selected_data {
			let metadata = &data.sets.get(&dataset_id).unwrap().metadata;
			for field_id in field_ids {
				let field = &metadata.fields[*field_id as usize];

				client_plot_metadata.lines.push( Line {
					r#type: "scattergl".to_string(),
					mode: "markers".to_string(),
					name: field.name.to_owned(),
				});
				client_plot_metadata.id_info.push( IdInfo {
					dataset_id: *dataset_id,
					field_id: *field_id,
				});
			}
		}
		let json = serde_json::to_string(&client_plot_metadata).unwrap();
		json
	}
	
	fn send_decode_info(&self, args: Vec<&str>, ctx: &mut ws::WebsocketContext<Self, WebServerData>) {
		trace!("sending decode info to client");
		if args.len() < 2 {warn!("can not send decode info without setid"); return; }
		if let Ok(set_id) = args[1].parse::<timeseries_interface::DatasetId>() {
			if let Some(fields) = self.selected_data.get(&set_id){
				let data = ctx.state().data.read().unwrap();
				let dataset = data.sets.get(&set_id).unwrap();
				let decode_info = dataset.get_decode_info(fields);
				std::mem::drop(data);
				let decode_info = bincode::serialize(&decode_info).unwrap();
				ctx.binary(Binary::from(decode_info));
			} else {
				warn!("tried access to unautorised or non existing dataset");
			}
		}
	}

	//fn send_data(&mut self, data: &Arc<RwLock<timeseries_interface::Data>>, ){
	fn send_data(&mut self, ctx: &mut ws::WebsocketContext<Self, WebServerData>){
		trace!("sending data to client");
		let now = Utc::now();
		let t_start= now - Duration::hours(196);
		let t_end = Utc::now();

		if self.compression_enabled {
			unimplemented!();
		} else {
			for (dataset_id, field_ids) in &self.selected_data {
				let mut data = ctx.state().data.write().unwrap();//todo clone for use in loop
				let dataset = data.sets.get_mut(dataset_id).unwrap();

				let mut read_state = dataset.prepare_read(t_start,t_end, field_ids).unwrap();//FIXME handle possible error!!!
				let n_bytes = read_state.bytes_to_read();
				std::mem::drop(dataset);
				std::mem::drop(data);

				debug!("read_state: {:?}", read_state);
				for package_numb in (0..divide_ceil(n_bytes,config::MAX_LINES_PER_PACKAGE as u64) ).rev() {
					//TODO some mechanisme to loop multiple get_data_chunk
					trace!("sending data packet numb: {}", package_numb);
					let mut data = ctx.state().data.write().unwrap();
					let dataset = data.sets.get_mut(dataset_id).unwrap();
					let buffer = dataset.get_data_chunk_uncompressed(
						&mut read_state,
						config::MAX_LINES_PER_PACKAGE,
						package_numb as u16,
						*dataset_id).unwrap();//FIXME handle possible error!!!
					std::mem::drop(dataset);
					std::mem::drop(data);

					ctx.binary(Binary::from(buffer ));
				}
			}
		};

	}
}

#[derive(Serialize)]
pub struct SetSliceDecodeInfo {
	pub field_lenghts: Vec<u8>,
	pub field_offsets: Vec<u8>,
	pub data_is_little_endian: bool,
}

/// Handler for `ws::Message`
impl StreamHandler<ws::Message, ws::ProtocolError> for WsSession {
	
	fn handle(&mut self, msg: ws::Message, ctx: &mut Self::Context) {
		// process websocket messages
		//println!("WS: {:?}", msg);
		match msg {
			ws::Message::Text(text) => {
				let m = text.trim();
				if m.starts_with('/') {
					let args: Vec<&str> = m.split_whitespace().collect();
					//println!("args: {:?}",args);
					match args[0] {
						//select uncompressed will case data to be send without compression
						//it is used until webassembly is relaiable
						"/select" => self.select_data(args, true),
						"/select_uncompressed" => self.select_data(args, false),

						"/sub" => self.subscribe(&ctx.state().websocket_addr),
						"/meta" => ctx.text(self.send_metadata(&ctx.state().data)),
						"/data" => self.send_data(ctx),

						//for use with webassembly only
						"/decode_info" => self.send_decode_info(args, ctx),

						_ => ctx.text(format!("!!! unknown command: {:?}", m)),
					}
				}
			} //handle other websocket commands
			ws::Message::Ping(msg) => ctx.pong(&msg),
			ws::Message::Binary(bin) => ctx.binary(bin),
			ws::Message::Close(_) => {
				ctx.state().websocket_addr.do_send(websocket_data_router::Disconnect {ws_session_id: self.ws_session_id,});
				ctx.stop();
			}
			_ => (),
		}
	}
}

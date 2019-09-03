use actix::*;
use log::{trace};

use actix_web_actors::ws;

use futures::Future;

use std::sync::{Arc, Mutex};

use crate::httpserver::Session;
use super::error_router;

// store data in here, it can then be accessed using self
pub struct WsSession {
	/// unique session id
	pub http_session_id: u16,
	pub ws_session_id: u16,
	pub router_addr: Addr<error_router::ErrorRouter>,
	pub session: Arc<Mutex<Session>>,
}

//TODO check if static needed
impl Actor for WsSession {
	//type Context = ws::WebsocketContext<Self, DataRouterState>;
	type Context = ws::WebsocketContext<Self>;

	//fn started<T: InnerState>(&mut self, ctx: &mut Self::Context) {
	fn started(&mut self, ctx: &mut Self::Context) {

		let ts_with_access = &self.session.lock().unwrap().db_entry.timeseries_with_access;
		let subscribed_errors = ts_with_access
			.iter().flat_map(|(set_id, auth)| {
			auth.iter().map(|auth| auth.as_ref()).map(move |field_id| {
				error_router::to_field_specific_key(*set_id, *field_id)
			}).chain(std::iter::once(error_router::to_field_specific_key(*set_id, u8::max_value())))
		}).collect();

		let addr = ctx.address();
		self.router_addr
            .send(error_router::Connect {
                addr: addr.recipient(),
                ws_session_id: self.ws_session_id,
				subscribed_errors,
            })
            .wait().unwrap();
	}

	fn stopping(&mut self, _ctx: &mut Self::Context) -> Running {
		// notify chat server
		self.router_addr
			.do_send(error_router::Disconnect { ws_session_id: self.ws_session_id });
		Running::Stop
	}
}


/// send messages to server if requested by dataserver
impl Handler<error_router::NewFormattedError> for WsSession {
	type Result = ();

	fn handle(&mut self, msg: error_router::NewFormattedError, ctx: &mut Self::Context) {
		trace!("client handler recieved signal there is new error");
		ctx.text(msg.error_message);
	}
}


/// Handler for `ws::Message`
impl StreamHandler<ws::Message, ws::ProtocolError> for WsSession {
	
	fn handle(&mut self, msg: ws::Message, _ctx: &mut Self::Context) {
		// process websocket messages
		println!("WS: {:?}", msg);
	}
}

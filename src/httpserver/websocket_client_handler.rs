use self::actix::Addr;
use self::actix::*;

extern crate actix;
extern crate actix_net;
extern crate actix_web;
extern crate actix_web_httpauth;

use self::actix_web::Error as wError;
use self::actix_web::Result as wResult;
use self::actix_web::{
	fs::NamedFile, http, http::Method, http::StatusCode, middleware, server, ws, App,
	AsyncResponder, Form, FutureResponse, HttpMessage, HttpRequest, HttpResponse, Responder,
};

use super::timeseries_interface;
use super::websocket_data_router;
use super::WebServerData;

// store data in here, it can then be accessed using self
pub struct WsSession {
	/// unique session id
	pub session_id: u16,
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
            //wait for response
            .into_actor(self)
            //process response in closure
            .then(|res, act, ctx| {
                fut::ok(())
            })
            .wait(ctx);
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

	fn handle(&mut self, msg: websocket_data_router::NewData, ctx: &mut Self::Context) {
		println!("websocket");
		//ctx.text(msg.0);
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
					let v: Vec<&str> = m.splitn(2, ' ').collect();
					match v[0] {
						"/Sub" => {
							if let Ok(source) = v[1].parse::<timeseries_interface::DatasetId>() {
								ctx.state()
									.websocket_addr
									.do_send(websocket_data_router::SubscribeToSource {
										session_id: self.session_id,
										set_id: 0,
									});
							} else {
								warn!("unknown source: {}", v[1]);
							}
						}
						"/name" => {}
						_ => ctx.text(format!("!!! unknown command: {:?}", m)),
					}
				}
			} //handle other websocket commands
			ws::Message::Ping(msg) => ctx.pong(&msg),
			ws::Message::Binary(bin) => ctx.binary(bin),
			ws::Message::Close(_) => {
				ctx.stop();
			}
			_ => (),
		}
	}
}

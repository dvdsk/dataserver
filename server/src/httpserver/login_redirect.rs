use std::task::{Context, Poll};

use actix_service::{Service, Transform};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::{http, Error, HttpResponse};

use futures::future::{ok, Either, Ready};

use log::info;

use actix_identity::RequestIdentity;

use crate::data_store::data_router::DataRouterState;
//example to mimic: https://github.com/actix/examples/blob/master/middleware/src/redirect.rs

#[derive(Default)]
pub struct CheckLogin {}

impl<S: Service<Req>, Req> Transform<S, Req> for CheckLogin
where
	S::Future: 'static,
{
	type Response = S::Response;
	type Error = Error;
	type InitError = S::Error;
	type Transform = CheckLoginMiddleware<S>;
	type Future = Ready<Result<Self::Transform, Self::InitError>>;

	fn new_transform(&self, service: S) -> Self::Future {
		ok(CheckLoginMiddleware { service })
	}
}

pub struct CheckLoginMiddleware<S> {
	service: S,
}

//TODO can we get data into the middleware? look at existing identityservice
fn is_logged_in(state: &DataRouterState, id: String) -> Result<(), ()> {
	if let Ok(id) = id.parse::<u16>() {
		//check if valid session (identity key contained in sessions)
		if state.sessions.read().unwrap().contains_key(&id) {
			Ok(())
		} else {
			Err(())
		}
	} else {
		Err(())
	}
}

impl<S: Service<Req>, Req> Service<Req> for CheckLoginMiddleware<S> {
	type Response = S::Response;
	type Error = Error;
	type Future = Either<S::Future, Result<Self::Response, Self::Error>>;

	fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.service.poll_ready(cx)
	}

	fn call(&mut self, req: ServiceRequest) -> Self::Future {
		// We only need to hook into the `start` for this middleware.

		if let Some(id) = req.get_identity() {
			let data = req.app_data::<DataRouterState>().unwrap();
			if is_logged_in(data, id).is_ok() {
				//let fut =
				Either::Left(self.service.call(req))
			} else {
				let redirect = "/login".to_owned() + req.path();
				Either::Right(Ok(req.into_response(
					HttpResponse::Found()
						.append_header((http::header::LOCATION, redirect))
						.finish()
						.into_body(), //TODO why comma? is needed?
				)))
			}
		} else {
			info!("could not get identity thus redirecting");
			let redirect = "/login".to_owned() + req.path();
			Either::Right(Ok(req.into_response(
				HttpResponse::Found()
					.append_header((http::header::LOCATION, redirect))
					.finish()
					.into_body(), //TODO why comma? is needed?
			)))
		}
	}
}

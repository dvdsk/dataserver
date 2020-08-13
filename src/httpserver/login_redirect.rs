use std::task::{Context, Poll};

use actix_service::{Service, Transform};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::{http, web, Error, HttpResponse};

use futures::future::{ok, Either, Ready};

use log::info;

use actix_identity::RequestIdentity;

use crate::data_store::data_router::DataRouterState;
//example to mimic: https://github.com/actix/examples/blob/master/middleware/src/redirect.rs

#[derive(Default)]
pub struct CheckLogin {}

impl<S, B> Transform<S> for CheckLogin
where
	S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
	S::Future: 'static,
{
	type Request = ServiceRequest;
	type Response = ServiceResponse<B>;
	type Error = Error;
	type InitError = ();
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
fn is_logged_in(state: &web::Data<DataRouterState>, id: String) -> Result<(), ()> {
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

impl<S, B> Service for CheckLoginMiddleware<S>
where
	S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
	S::Future: 'static,
{
	type Request = ServiceRequest;
	type Response = ServiceResponse<B>;
	type Error = Error;
	type Future = Either<S::Future, Ready<Result<Self::Response, Self::Error>>>;
	//type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>>>>;

	fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
		self.service.poll_ready(cx)
	}

	fn call(&mut self, req: ServiceRequest) -> Self::Future {
		// We only need to hook into the `start` for this middleware.

		if let Some(id) = req.get_identity() {
			//let id = req.get_identity().unwrap();
			let data: web::Data<DataRouterState> = req.app_data().unwrap();
			if is_logged_in(&data, id).is_ok() {
				//let fut =
				Either::Left(self.service.call(req))
			} else {
				let redirect = "/login".to_owned() + req.path();
				Either::Right(ok(req.into_response(
					HttpResponse::Found()
						.header(http::header::LOCATION, redirect)
						.finish()
						.into_body(), //TODO why comma? is needed?
				)))
			}
		} else {
			info!("could not get identity thus redirecting");
			let redirect = "/login".to_owned() + req.path();
			Either::Right(ok(req.into_response(
				HttpResponse::Found()
					.header(http::header::LOCATION, redirect)
					.finish()
					.into_body(), //TODO why comma? is needed?
			)))
		}
	}
}

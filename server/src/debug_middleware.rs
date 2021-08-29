use std::pin::Pin;
use std::task::{Context, Poll};

use actix_service::{Service, Transform};
use actix_web::{dev::ServiceRequest, dev::ServiceResponse, Error};
use futures::future::{ok, Ready};
use futures::Future;

// There are two steps in middleware processing.
// 1. Middleware initialization, middleware factory gets called with
//    next service in chain as parameter.
// 2. Middleware's call method gets called with normal request.
#[allow(dead_code)]
pub struct SayHiTransform;

impl<S: Service<Req>, Req> Transform<S, Req> for SayHiTransform
where
	Req: actix_web::dev::ResourcePath,
{
	type Response = S::Response;
	type Error = S::Error;
	type InitError = S::Error;
	type Transform = SayHiMiddleware<S>;
	type Future = Ready<Result<Self::Transform, Self::InitError>>;

	fn new_transform(&self, service: S) -> Self::Future {
		futures::future::ready(Ok(SayHiMiddleware { service }))
	}
}

pub struct SayHiMiddleware<S> {
	service: S,
}

impl<S: Service<Req>, Req> Service<Req> for SayHiMiddleware<S>
where
	Req: actix_web::dev::ResourcePath,
{
	type Response = S::Response;
	type Error = S::Error;
	type Future = S::Future;

    actix_service::forward_ready!(service);	

	fn call(&self, req: Req) -> Self::Future {
		println!("You requested: {}", req.path());
		self.service.call(req)
	}
}

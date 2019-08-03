use actix_service::{Service, Transform};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::{http, Error, HttpResponse};
use actix_web::HttpRequest;
use futures::future::{ok, Either, FutureResult};
use futures::Poll;

use actix_identity::{Identity, CookieIdentityPolicy, IdentityService};
//example to mimic: https://github.com/actix/examples/blob/master/middleware/src/redirect.rs

#[derive(Default)]
pub struct CheckLogin;

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
    type Future = FutureResult<Self::Transform, Self::InitError>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(CheckLoginMiddleware { service })
    }
}

pub struct CheckLoginMiddleware<S> {
    service: S,
}

fn is_logged_in<T: InnerState>(state: web::Data<T>, id: Identity) -> bool {
	let id = req.identity().map_err(|| false)?
	            .parse().map_err(|| false)?
	
	//check if valid session (identity key contained in sessions)
	state.inner_state().sessions.read().unwrap().contains_key(id)
}

impl<S, B> Service for CheckLoginMiddleware<S>
where
    S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
{
    type Request = ServiceRequest;
    type Response = ServiceResponse<B>;
    type Error = Error;
    type Future = Either<S::Future, FutureResult<Self::Response, Self::Error>>;

    fn poll_ready(&mut self) -> Poll<(), Self::Error> {
        self.service.poll_ready()
    }

    fn call(&mut self, req: ServiceRequest, id: Identity, state: web::Data<T>) -> Self::Future {
        // We only need to hook into the `start` for this middleware.

		if let Some(id) = req.identity() {
            //check if valid session
            if req.state().inner_state().sessions.read().unwrap().contains_key(&id.parse().unwrap()) {
				return Ok(middleware::Started::Done);
			}
		}

        if is_logged_in() {
            Either::A(self.service.call(req))
        } else {
			let path = req.path();
            Either::B(ok(req.into_response(
                HttpResponse::Found()
                    .header(http::header::LOCATION, "/login".to_owned()+path)
                    .finish()
                    .into_body(), //TODO why comma? is needed?
            )))
        }
    }
}
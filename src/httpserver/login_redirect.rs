use actix_service::{Service, Transform};
use actix_web::dev::{ServiceRequest, ServiceResponse};
use actix_web::{http, Error, HttpResponse, web};
use actix_web::HttpRequest;
use actix_web::FromRequest;

use futures::future::{ok, Either, FutureResult};
use futures::Poll;

use super::InnerState;

use actix_identity::{Identity, CookieIdentityPolicy, IdentityService, RequestIdentity};
//example to mimic: https://github.com/actix/examples/blob/master/middleware/src/redirect.rs

#[derive(Default)]
pub struct CheckLogin<T>{
    pub phantom: std::marker::PhantomData<T>,
}

impl<S, T, B> Transform<S> for CheckLogin<T>
where
    T: InnerState + 'static,
    S: Service<Request = ServiceRequest, Response = ServiceResponse<B>, Error = Error>,
    S::Future: 'static,
{
    type Request = ServiceRequest;
    type Response = ServiceResponse<B>;
    type Error = Error;
    type InitError = ();
    type Transform = CheckLoginMiddleware<S, T>;
    type Future = FutureResult<Self::Transform, Self::InitError>;

    fn new_transform(&self, service: S) -> Self::Future {
        ok(CheckLoginMiddleware { service, phantom: std::marker::PhantomData })
    }
}

pub struct CheckLoginMiddleware<S,T> {
    service: S,
    phantom: std::marker::PhantomData<T>,
}

//TODO can we get data into the middleware? look at existing identityservice
fn is_logged_in<T: InnerState>(state: &web::Data<T>, id: String) -> Result<(),()> {
    if let Ok(id) = id.parse::<u16>(){
        //check if valid session (identity key contained in sessions)
        if state.inner_state().sessions.read().unwrap().contains_key(&id){
            Ok(())
        } else {
            Err(())
        }
    } else {Err(()) }

}

impl<S, T, B> Service for CheckLoginMiddleware<S,T>
where
    T: InnerState + 'static,
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

    fn call(&mut self, req: ServiceRequest) -> Self::Future {
        // We only need to hook into the `start` for this middleware.

        if let Some(id) = req.get_identity() {
            let data: web::Data<T> = req.app_data().unwrap();
            if is_logged_in(&data, id).is_ok() {
                Either::A(self.service.call(req))
            } else {
                let redirect = "/login".to_owned()+req.path();
                Either::B(ok(req.into_response(
                    HttpResponse::Found()
                        .header(http::header::LOCATION, redirect)
                        .finish()
                        .into_body(), //TODO why comma? is needed?
                )))
            }
        } else {
                let redirect = "/login".to_owned()+req.path();
            Either::B(ok(req.into_response(
                HttpResponse::Found()
                    .header(http::header::LOCATION, redirect)
                    .finish()
                    .into_body(), //TODO why comma? is needed?        
            )))   
        }
    }
}
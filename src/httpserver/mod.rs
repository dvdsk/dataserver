mod login_redirect;
mod dynamic_pages;
pub mod data_router_ws_client;
mod error_router_ws_client;
mod handlers;
pub mod utility;

use std::thread;
use std::sync::{mpsc};
use std::path::Path;

use actix_web::{HttpServer,App, web};
use actix_identity::{CookieIdentityPolicy, IdentityService};
use actix_files as fs;

use crate::error::DataserverError as Error;
use crate::databases::User;
use crate::data_store::{data_router::DataRouterState};

use crate::bot;
use login_redirect::CheckLogin;

pub struct Session { 
	db_entry: User,
    //add more temporary user specific data as needed
}

pub fn start(data_router_state: DataRouterState, key_dir: &Path,
    port: u16, domain: String)
     -> Result<actix_web::dev::Server,Error> {

   let tls_config = utility::make_tls_config(&domain, key_dir)?;
   let cookie_key = utility::make_random_cookie_key();
   let token = data_router_state.bot_token.clone();

   let (tx, rx) = mpsc::channel();

   thread::spawn(move || {
       let sys = actix::System::new("http-server");

       let web_server = HttpServer::new(move || {
           // data the webservers functions have access to
           let data = actix_web::web::Data::new(data_router_state.clone());
           
           App::new()
               .app_data(data)
               .wrap(IdentityService::new(
                   CookieIdentityPolicy::new(&cookie_key[..])
                   .domain(&domain)
                   .name("auth-cookie")
                   .path("/")
                   .secure(true), 
               ))
               //.wrap(debug_middleware::SayHi) //prints all requested URLs
               .service(
                   web::scope("/login")
                       .service(web::resource(r"/{path}")
                           .route(web::post().to(handlers::login_get_and_check))
                           .route(web::get().to(handlers::login_page))
               ))
               .service(web::resource("/post_data").to(handlers::new_data_post))
               .service(web::resource("/post_error").to(handlers::new_error_post))
               .service(web::resource(&format!("/{}", &token)).to(bot::handle_webhook))

               .service(
                   web::scope("/")
                       .wrap(CheckLogin {})
                       .service(web::resource("").to(handlers::index))
                       .service(web::resource("index").to(handlers::index))
                       .service(web::resource("ws/data/").to(handlers::data_router_ws_index))
                       .service(web::resource("ws/error").to(handlers::error_router_ws_index))
                       .service(web::resource("logout").to(handlers::logout))
                       .service(web::resource("plot").to(handlers::plot_data))
                       .service(web::resource("list_data").to(dynamic_pages::list_data))
                       .service(web::resource("settings.html")
                           .route(web::get().to(dynamic_pages::settings_page))
                           .route(web::post().to(handlers::set_telegram_id_post))
                       )
                       //for all other urls we try to resolve to static files in the "web" dir
                       .service(fs::Files::new("", "./web/"))
               )
           })
       // WARNING TLS IS NEEDED FOR THE LOGIN SYSTEM TO FUNCTION
       .bind_rustls(&format!("0.0.0.0:{}", port), tls_config).unwrap()
       .shutdown_timeout(5)    // shut down 5 seconds after getting the signal to shut down
       .run(); // end of App::new()


       let _ = tx.send(web_server);
       let _ = sys.run();
   }); //httpserver closure

   let web_handle = rx.recv().unwrap();
   Ok(web_handle)
}
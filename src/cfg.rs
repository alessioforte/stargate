use crate::data::state::State;
use crate::errors::{HttpError, PayloadError};
use actix_web::error::JsonPayloadError;
use actix_web::http::header::CONTENT_TYPE;
use actix_web::web::ServiceConfig;
use actix_web::{web, web::Data, HttpRequest};

pub fn app_data(cfg: &mut ServiceConfig) {
    let state = State::init();
    let data = Data::new(state);

    cfg.app_data(
        web::JsonConfig::default()
            .content_type(|mime| mime == mime::APPLICATION_JSON)
            .error_handler(|err, req: &HttpRequest| match err {
                JsonPayloadError::ContentType => match req.headers().get(CONTENT_TYPE) {
                    Some(content_type) => HttpError::InvalidContentType(
                        content_type.to_str().unwrap_or("unknown").to_string(),
                        vec![mime::APPLICATION_JSON.to_string()],
                    )
                    .into(),
                    None => HttpError::MissingContentType(vec![mime::APPLICATION_JSON.to_string()])
                        .into(),
                },
                err => PayloadError::from(err).into(),
            }),
    )
    .app_data(data.clone());
}

pub const LOGO: &str = "
    .d88888b.   dP                                         dP
    88.         88                                         88
    'Y88888b. d8888P .d8888b. 88d888b. .d8888b. .d8888b. d8888P .d8888b.
          '8b   88   88'  '88 88'  '88 88'  '88 88'  '88   88   88ooood8
    d8'   .8P   88   88.  .88 88       88.  .88 88.  .88   88   88.  ...
     Y88888P    dP   '88888P8 dP       '8888P88 '88888P8   dP   '88888P'
                                            .88
                                        d8888P
";

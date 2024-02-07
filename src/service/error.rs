use std::convert::Infallible;
use std::ops::FromResidual;
use std::result::Result as StdResult;

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use paste::paste;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
pub struct Error {
    code: u32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    details: Option<String>,

    #[serde(skip)]
    status_code: StatusCode,
}

macro_rules! define_error {
    ($name:ident, $code:literal, $status:expr, $msg:literal) => {
        impl Error {
            #[inline]
            pub fn $name() -> Self {
                // Eliminate dead code warning when we use either of these two constructors.
                paste! {
                    if false {
                        Self::[<$name _with_details>]("");
                    }
                }

                Self {
                    code: $code,
                    message: $msg.into(),
                    details: None,
                    status_code: $status,
                }
            }

            paste! {
                #[inline]
                pub fn [<$name _with_details>]<S: Into<String>>(details: S) -> Self {
                    // Eliminate dead code warning when we use either of these two constructors.
                    if false {
                        Self::$name();
                    }

                    Self {
                        code: $code,
                        message: $msg.into(),
                        details: Some(details.into()),
                        status_code: $status,
                    }
                }
            }
        }
    };
}

define_error!(
    unsupported_encoding,
    1,
    StatusCode::BAD_REQUEST,
    "body has an unsupported encoding"
);
define_error!(
    failed_to_spawn,
    2,
    StatusCode::INTERNAL_SERVER_ERROR,
    "failed to spawn the process"
);
define_error!(
    job_not_found,
    3,
    StatusCode::NOT_FOUND,
    "the specified job is not found"
);
define_error!(
    job_has_finished,
    4,
    StatusCode::GONE,
    "the specified job is already finished"
);
define_error!(
    api_not_found,
    5,
    StatusCode::NOT_FOUND,
    "the API endpoint is not found"
);

pub struct Result<T>(StdResult<T, Error>);

impl<T: Serialize> IntoResponse for Result<T> {
    fn into_response(self) -> Response {
        trait OptionExt {
            fn is_empty(&self) -> bool;
        }

        impl<T> OptionExt for Option<T> {
            default fn is_empty(&self) -> bool {
                self.is_none()
            }
        }

        impl OptionExt for Option<()> {
            default fn is_empty(&self) -> bool {
                true
            }
        }

        #[derive(Serialize)]
        struct ResponseBody<T> {
            success: bool,
            #[serde(skip_serializing_if = "OptionExt::is_empty")]
            payload: Option<T>,
            #[serde(skip_serializing_if = "OptionExt::is_empty")]
            error: Option<Error>,
        }

        let (body, status_code) = match self.0 {
            Ok(value) => (
                ResponseBody {
                    success: true,
                    payload: Some(value),
                    error: None,
                },
                StatusCode::OK,
            ),
            Err(err) => {
                let status_code = err.status_code;

                (
                    ResponseBody {
                        success: false,
                        payload: None,
                        error: Some(err),
                    },
                    status_code,
                )
            }
        };

        let mut response = Json(body).into_response();
        *response.status_mut() = status_code;

        response
    }
}

impl<T> From<StdResult<T, Error>> for Result<T> {
    #[inline]
    fn from(value: StdResult<T, Error>) -> Self {
        Self(value)
    }
}

impl<T> From<Error> for Result<T> {
    #[inline]
    fn from(value: Error) -> Self {
        Err(value).into()
    }
}

impl<T> FromResidual<StdResult<Infallible, Error>> for Result<T> {
    #[inline]
    fn from_residual(residual: StdResult<Infallible, Error>) -> Self {
        Err(residual.unwrap_err()).into()
    }
}

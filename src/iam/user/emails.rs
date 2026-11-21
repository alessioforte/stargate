use crate::err::{ErrorCode, ErrorResponse};

/// Checks an address against the `users.email` column before it reaches the
/// repository, so callers answer `400` rather than surfacing the storage
/// invariant as a server error.
pub(crate) fn ensure_email_fits_column(email: &str) -> Result<(), ErrorResponse> {
    if db::ent::email_fits_column(email) {
        Ok(())
    } else {
        Err(ErrorResponse::new(ErrorCode::RequestInvalid).with_param("field", "email"))
    }
}

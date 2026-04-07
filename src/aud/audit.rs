macro_rules! creation {
    ($ctx:expr) => {
        if let Some(service) = crate::aud::get_audit_service() {
            service.log($ctx.build_audit(ActionType::Create));
        } else {
            tracing::error!("AuditService not initialized");
        }
    };
}

macro_rules! modification {
    ($ctx:expr) => {
        if let Some(service) = crate::aud::get_audit_service() {
            service.log($ctx.build_audit(ActionType::Update));
        } else {
            tracing::error!("AuditService not initialized");
        }
    };
}

macro_rules! deletion {
    ($ctx:expr) => {
        if let Some(service) = crate::aud::get_audit_service() {
            service.log($ctx.build_audit(ActionType::Delete));
        } else {
            tracing::error!("AuditService not initialized");
        }
    };
}

pub(crate) use creation;
pub(crate) use deletion;
pub(crate) use modification;

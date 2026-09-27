use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AppointmentWorkflow {
    Scheduling,
    Confirmed,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OperationalNotice {
    pub tenant_id: String,
    pub domain: String,
    pub audience: &'static str,
    pub action: &'static str,
}

pub fn decide_notification(
    tenant_id: &str,
    domain: &str,
    workflow: &AppointmentWorkflow,
    domain_verified: bool,
) -> Option<OperationalNotice> {
    if !domain_verified || workflow != &AppointmentWorkflow::Scheduling {
        return None;
    }

    Some(OperationalNotice {
        tenant_id: tenant_id.to_owned(),
        domain: domain.to_owned(),
        audience: "clinic_operations",
        action: "enable_appointment_portal",
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verified_scheduling_domain_emits_staff_only_notice() {
        let notice = decide_notification(
            "clinic-42",
            "appointments.example.org",
            &AppointmentWorkflow::Scheduling,
            true,
        )
        .expect("verified scheduling domain should notify operations");

        assert_eq!(notice.audience, "clinic_operations");
        assert_eq!(notice.action, "enable_appointment_portal");
        assert_eq!(notice.tenant_id, "clinic-42");
    }

    #[test]
    fn pending_domain_does_not_emit_a_notice() {
        assert_eq!(
            decide_notification(
                "clinic-42",
                "appointments.example.org",
                &AppointmentWorkflow::Scheduling,
                false,
            ),
            None
        );
    }
}

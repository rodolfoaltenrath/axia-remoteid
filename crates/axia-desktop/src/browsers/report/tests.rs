use super::*;
use crate::browsers::{Family, MODULE_NAME};

fn target(name: &str) -> Target {
    Target {
        family: Family::Firefox,
        label: "Firefox — Trabalho".into(),
        database: PathBuf::from(format!("/laboratory/{name}")),
        create: false,
        firefox_default: None,
        installations: Vec::new(),
    }
}
fn success(target: &Target) -> Registration {
    Registration {
        browser: target.family,
        profile: target.database.clone(),
        module: MODULE_NAME,
        registered_now: false,
        migrated_legacy_module: false,
        module_loaded_by_nss: true,
        certificate_count: 2,
        backup_directory: None,
        remote_signature_requested: false,
    }
}

#[test]
fn batch_keeps_success_and_attempts_later_profiles_after_failure_once() {
    let targets = vec![
        target("first"),
        target("second"),
        target("third"),
        target("first"),
    ];
    let mut attempted = Vec::new();
    let mut progress = Vec::new();
    let report = configure_with(
        targets,
        |target| progress.push(target.database.clone()),
        |target| {
            attempted.push(target.database.clone());
            if target.database.ends_with("second") {
                anyhow::bail!("NSS error -999: synthetic failure");
            }
            Ok(success(target))
        },
    );
    assert_eq!(progress, attempted);
    assert_eq!(attempted.len(), 3);
    assert_eq!(report.status, SetupStatus::Partial);
    assert!(!report.successful());
    assert_eq!(
        report
            .results
            .iter()
            .map(|result| result.status)
            .collect::<Vec<_>>(),
        [
            TargetStatus::Configured,
            TargetStatus::Failed,
            TargetStatus::Configured
        ]
    );
    assert!(
        report.results[1]
            .failure
            .as_ref()
            .unwrap()
            .message
            .contains("NSS error -999")
    );
    assert!(report.summary().contains("Reabra"));
    let json = serde_json::to_value(report).unwrap();
    assert_eq!(json["schema_version"], 1);
    assert_eq!(json["status"], "partial");
    assert_eq!(json["results"][0]["registration"]["registered_now"], false);
    assert_eq!(json["results"][1]["failure"]["kind"], "configuration");
    assert_eq!(
        json["results"][2]["target"]["database"],
        "/laboratory/third"
    );
}

#[test]
fn total_success_failure_cancellation_and_no_targets_remain_distinct() {
    let configured = configure_with(vec![target("first")], |_| {}, |target| Ok(success(target)));
    assert!(configured.successful());
    assert_eq!(configured.status, SetupStatus::Configured);
    let failed = configure_with(
        vec![target("first")],
        |_| {},
        |_| anyhow::bail!("synthetic failure"),
    );
    assert_eq!(failed.status, SetupStatus::Failed);
    let cancelled = SetupReport::empty(SetupStatus::Cancelled, Vec::new());
    let absent = configure_with(Vec::new(), |_| panic!("no progress"), |_| panic!("no NSS"));
    assert_eq!(cancelled.status, SetupStatus::Cancelled);
    assert_eq!(absent.status, SetupStatus::NoTargets);
    for report in [failed, cancelled, absent] {
        assert!(!report.successful());
        assert!(!report.summary().contains("Reabra"));
    }
}

#[test]
fn blockers_are_typed_and_never_inferred_from_message_substrings() {
    for kind in [
        FailureKind::BrowserOpen,
        FailureKind::ProfileBusy,
        FailureKind::InspectionUnavailable,
        FailureKind::ProfileNotInitialized,
    ] {
        let result = TargetResult::from_registration(
            target("first"),
            Err(anyhow::Error::new(Blocked(kind)).context("synthetic context")),
        );
        assert_eq!(result.status, TargetStatus::NeedsAction);
        assert_eq!(result.failure.unwrap().kind, kind);
    }
    let result = TargetResult::from_registration(
        target("first"),
        Err(anyhow::anyhow!(
            "Feche o navegador: simulated unrelated failure"
        )),
    );
    assert_eq!(result.status, TargetStatus::Failed);
    assert_eq!(result.failure.unwrap().kind, FailureKind::Configuration);
}

#[test]
fn suspended_and_failed_recovery_preserve_nss_cause_and_backup_without_claiming_restore() {
    for state in [
        RecoveryState::Suspended,
        RecoveryState::Failed,
        RecoveryState::Restored,
        RecoveryState::NotNeeded,
    ] {
        let error = anyhow::anyhow!("NSS synthetic code -8018").context(Recovery {
            state,
            backup_directory: PathBuf::from("/laboratory/backup"),
            reason: (state == RecoveryState::Suspended).then_some(FailureKind::BrowserOpen),
            detail: format!("recovery {state:?}"),
        });
        let result = TargetResult::from_registration(target("first"), Err(error));
        let failure = result.failure.as_ref().unwrap();
        assert_eq!(failure.recovery.as_ref().unwrap().state, state);
        assert!(failure.message.contains("NSS synthetic code -8018"));
        assert!(result.text(Path::new("/laboratory")).contains("~/backup"));
        assert_eq!(
            result.status,
            if state == RecoveryState::Suspended {
                TargetStatus::NeedsAction
            } else {
                TargetStatus::Failed
            }
        );
        assert!(
            !SetupReport::from_results(vec![result])
                .summary()
                .contains("Reabra")
        );
    }
}

#[test]
fn same_labels_in_distinct_profiles_are_distinct_targets() {
    let first = TargetResult::from_registration(target("first"), Ok(success(&target("first"))));
    let second = TargetResult::from_registration(target("second"), Ok(success(&target("second"))));
    assert_eq!(first.target.label, second.target.label);
    assert!(!first.same_target(&second));
    assert!(first.same_target(&first.clone()));
    let mut other_family = first.clone();
    other_family.target.family = Family::Chromium;
    assert!(!first.same_target(&other_family));
}

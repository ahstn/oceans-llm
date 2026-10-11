use gateway_skills::{BundleLimits, SkillDetail, SkillSummary, SkillUploadResponse};
use time::OffsetDateTime;
use uuid::Uuid;

#[test]
fn timestamps_are_rfc3339_strings_and_upload_version_is_explicit() {
    let upload = SkillUploadResponse {
        detail: SkillDetail {
            skill: SkillSummary {
                id: Uuid::nil(),
                namespace: "user".into(),
                name: "review".into(),
                owner_user_id: Uuid::nil(),
                description: "Review code.".into(),
                default_version: 1,
                latest_version: 3,
                created_at: OffsetDateTime::UNIX_EPOCH,
                updated_at: OffsetDateTime::UNIX_EPOCH,
            },
            versions: vec![],
        },
        uploaded_version: 2,
    };
    let value = serde_json::to_value(&upload).unwrap();
    assert_eq!(value["skill"]["created_at"], "1970-01-01T00:00:00Z");
    assert_eq!(value["uploaded_version"], 2);
    assert!(value.get("detail").is_none());
    assert_eq!(
        serde_json::from_value::<SkillUploadResponse>(value).unwrap(),
        upload
    );
}

#[test]
fn partial_limits_use_defaults_and_reject_unknown_options() {
    let limits: BundleLimits = serde_json::from_str(r#"{"max_files":5}"#).unwrap();
    assert_eq!(limits.max_files, 5);
    assert_eq!(
        limits.max_archive_bytes,
        BundleLimits::default().max_archive_bytes
    );
    assert!(serde_json::from_str::<BundleLimits>(r#"{"max_file":5}"#).is_err());
}

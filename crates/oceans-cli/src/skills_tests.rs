use std::{
    collections::BTreeMap,
    fs,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

use axum::{
    Json, Router,
    body::Bytes,
    extract::Query,
    routing::{get, post},
};
use gateway_skills::{SkillNamespace, SkillUploadResponse, pack_directory};
use time::OffsetDateTime;
use uuid::Uuid;

use super::*;

struct Server(tokio::task::JoinHandle<()>);

impl Drop for Server {
    fn drop(&mut self) {
        self.0.abort();
    }
}

async fn server(router: Router) -> (Client, Server) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    (
        Client::new(url.parse().unwrap(), "gwk_test.secret").unwrap(),
        Server(task),
    )
}

fn fixture() -> (tempfile::TempDir, ValidatedBundle, SkillDetail) {
    let directory = tempfile::tempdir().unwrap();
    let skill_directory = directory.path().join("review");
    fs::create_dir(&skill_directory).unwrap();
    fs::write(
        skill_directory.join("SKILL.md"),
        "---\nname: review\ndescription: Review code\n---\nReview the code carefully.\n",
    )
    .unwrap();
    let bundle = pack_directory(&skill_directory, &BundleLimits::default()).unwrap();
    let summary = SkillVersionSummary {
        version: 1,
        sha256: bundle.sha256.clone(),
        archive_bytes: bundle.archive.len() as u64,
        extracted_bytes: bundle.extracted_bytes,
        file_count: bundle.files.len() as u32,
        created_at: OffsetDateTime::UNIX_EPOCH,
    };
    let detail = SkillDetail {
        skill: SkillSummary {
            id: Uuid::new_v4(),
            namespace: "alice".into(),
            name: "review".into(),
            owner_user_id: Uuid::new_v4(),
            description: "Review code".into(),
            default_version: 1,
            latest_version: 1,
            created_at: OffsetDateTime::UNIX_EPOCH,
            updated_at: OffsetDateTime::UNIX_EPOCH,
        },
        versions: vec![summary],
    };
    (directory, bundle, detail)
}

#[tokio::test]
async fn download_pins_summary_version_and_digest_without_fetching_preview() {
    let (_directory, bundle, detail) = fixture();
    let archive_path = format!("/api/v1/skills/{}/versions/1/archive", detail.skill.id);
    let expected = bundle.archive.clone();
    let router = Router::new()
        .route(
            "/api/v1/skills/limits",
            get(|| async { Json(BundleLimits::default()) }),
        )
        .route(
            "/api/v1/skills/by-name/alice/review",
            get(move || {
                let detail = detail.clone();
                async { Json(detail) }
            }),
        )
        .route(
            &archive_path,
            get(move || {
                let archive = bundle.archive.clone();
                async { archive }
            }),
        );
    let (client, _server) = server(router).await;
    let fetched = fetch_archive(
        &client,
        &VersionArgs {
            skill: "alice/review".into(),
            version: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(fetched.bytes, expected);
    assert_eq!(fetched.version.version, 1);
    assert_eq!(fetched.bundle.manifest.name, "review");
}

#[tokio::test]
async fn upload_appends_edited_installed_skill_without_installer_record() {
    let (_directory, bundle, detail) = fixture();
    let installed_directory = tempfile::tempdir().unwrap();
    let record = InstallRecord {
        namespace: "alice".into(),
        skill_id: detail.skill.id,
        version: 1,
        sha256: bundle.sha256.clone(),
    };
    let installed =
        install::install_bundle(&bundle, &record, installed_directory.path(), false).unwrap();
    fs::write(
        installed.join("SKILL.md"),
        format!("{}\nAn edited instruction.\n", bundle.instructions),
    )
    .unwrap();
    let uploaded = Arc::new(AtomicUsize::new(0));
    let observed = uploaded.clone();
    let namespace = SkillNamespace {
        handle: "alice".into(),
        user_id: detail.skill.owner_user_id,
    };
    let upload_path = format!("/api/v1/skills/{}/versions", detail.skill.id);
    let mut response_detail = detail.clone();
    response_detail.skill.latest_version = 9; // A concurrent upload may already have advanced latest.
    let response = SkillUploadResponse {
        detail: response_detail,
        uploaded_version: 2,
    };
    let router = Router::new()
        .route(
            "/api/v1/skills/limits",
            get(|| async { Json(BundleLimits::default()) }),
        )
        .route(
            "/api/v1/skills/namespace",
            get(move || {
                let value = namespace.clone();
                async { Json(value) }
            }),
        )
        .route(
            "/api/v1/skills/by-name/alice/review",
            get(move || {
                let value = detail.clone();
                async { Json(value) }
            }),
        )
        .route(
            &upload_path,
            post(move |bytes: Bytes| {
                let response = response.clone();
                let observed = observed.clone();
                async move {
                    let received = inspect_archive(&bytes, &BundleLimits::default()).unwrap();
                    assert_eq!(received.manifest.name, "review");
                    assert!(received.instructions.contains("An edited instruction."));
                    assert!(!received.contents.contains_key(install::INSTALL_RECORD));
                    observed.fetch_add(1, Ordering::Relaxed);
                    Json(response)
                }
            }),
        );
    let (client, _server) = server(router).await;
    upload(&client, &installed, true).await.unwrap();
    assert_eq!(uploaded.load(Ordering::Relaxed), 1);
}

#[test]
fn zip_upload_rejects_installer_metadata_and_directory_keeps_invalid_metadata() {
    let (directory, _bundle, _detail) = fixture();
    let source = directory.path().join("review");
    let record_path = source.join(install::INSTALL_RECORD);
    fs::write(&record_path, "user content that is not an install record").unwrap();
    assert!(read_upload(&source, &BundleLimits::default()).is_err());
    assert_eq!(
        fs::read_to_string(&record_path).unwrap(),
        "user content that is not an install record"
    );
    let archive = pack_directory(&source, &BundleLimits::default()).unwrap();
    let zip_path = directory.path().join("review.zip");
    fs::write(&zip_path, archive.archive).unwrap();
    assert!(
        read_upload(&zip_path, &BundleLimits::default())
            .unwrap_err()
            .to_string()
            .contains("reserved installer file")
    );
}

#[tokio::test]
async fn download_applies_the_registry_configured_archive_limit() {
    let (_directory, bundle, detail) = fixture();
    let archive_path = format!("/api/v1/skills/{}/versions/1/archive", detail.skill.id);
    let router = Router::new()
        .route(
            "/api/v1/skills/limits",
            get(|| async {
                Json(BundleLimits {
                    max_archive_bytes: 1,
                    ..BundleLimits::default()
                })
            }),
        )
        .route(
            "/api/v1/skills/by-name/alice/review",
            get(move || {
                let detail = detail.clone();
                async { Json(detail) }
            }),
        )
        .route(
            &archive_path,
            get(move || {
                let archive = bundle.archive.clone();
                async { archive }
            }),
        );
    let (client, _server) = server(router).await;
    let error = fetch_archive(
        &client,
        &VersionArgs {
            skill: "alice/review".into(),
            version: None,
        },
    )
    .await
    .err()
    .unwrap();
    assert!(error.to_string().contains("1-byte limit"));
}

#[tokio::test]
async fn list_fetches_every_page() {
    let (_directory, _bundle, detail) = fixture();
    let offsets = Arc::new(Mutex::new(Vec::new()));
    let observed = offsets.clone();
    let router = Router::new().route(
        "/api/v1/skills",
        get(move |Query(query): Query<BTreeMap<String, String>>| {
            let observed = observed.clone();
            let summary = detail.skill.clone();
            async move {
                let offset: usize = query["offset"].parse().unwrap();
                observed.lock().unwrap().push(offset);
                assert_eq!(query["limit"], "100");
                assert_eq!(query["namespace"], "alice");
                Json(vec![summary; if offset == 0 { 100 } else { 1 }])
            }
        }),
    );
    let (client, _server) = server(router).await;
    list(&client, Some("alice".into()), true).await.unwrap();
    assert_eq!(*offsets.lock().unwrap(), vec![0, 100]);
}

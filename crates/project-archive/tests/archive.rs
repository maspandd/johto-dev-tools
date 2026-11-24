use std::io::{Cursor, Write};
use std::net::{IpAddr, Ipv4Addr};

use chrono::{TimeZone, Utc};
use project_archive::{
    ArchiveError, DatasetFile, EnvironmentFile, FORMAT_VERSION, Limits, ProjectBundle, ProjectFile,
    ResourceEntry, read, write,
};
use serde_json::{Value, json};
use workbench_core::BindAddress;
use workbench_domain::{
    DataSchema, Endpoint, FieldKind, FieldSpec, Locale, Method, MockResponse, PathTemplate,
    Resource, SeedSpec,
};
use zip::write::SimpleFileOptions;

fn bundle() -> ProjectBundle {
    let mut list = Endpoint::new(
        "List users",
        Method::Get,
        PathTemplate::parse("/users").unwrap(),
    );
    list.response = MockResponse::json(200, json!([{ "id": "{{ uuid }}" }]));
    let mut get = Endpoint::new(
        "Get user / by id",
        Method::Get,
        PathTemplate::parse("/users/:id").unwrap(),
    );
    get.enabled = false;

    let mut products = Resource::new("products", PathTemplate::parse("/products").unwrap());
    products.schema = DataSchema::new(vec![FieldSpec::new("name", FieldKind::Company)]);
    products.seed = Some(SeedSpec {
        count: 5,
        seed: 7,
        locale: Locale::Id,
    });
    let reviews = Resource::new("reviews", PathTemplate::parse("/reviews").unwrap());

    ProjectBundle {
        project: ProjectFile {
            name: "Shop".into(),
            description: "Demo".into(),
            base_path: Some("/api".into()),
            default_headers: [("X-Env".to_owned(), "dev".to_owned())].into(),
        },
        environments: vec![
            EnvironmentFile {
                name: "Development".into(),
                base_url: Some("http://localhost:4010".into()),
                default_headers: Default::default(),
                variables: [("API_VERSION".to_owned(), "1".to_owned())].into(),
                bind: BindAddress::loopback(4010),
                secret_names: vec!["API_TOKEN".into()],
            },
            EnvironmentFile {
                name: "Testing".into(),
                base_url: None,
                default_headers: Default::default(),
                variables: Default::default(),
                bind: BindAddress::loopback(4011),
                secret_names: vec![],
            },
        ],
        endpoints: vec![list, get],
        resources: vec![
            ResourceEntry {
                resource: products,
                records: Some(vec![
                    json!({ "id": "a", "name": "Acme" }),
                    json!({ "id": "b", "name": "Globex" }),
                ]),
            },
            ResourceEntry {
                resource: reviews,
                records: None,
            },
        ],
        datasets: vec![DatasetFile {
            name: "Customers".into(),
            description: String::new(),
            schema: DataSchema::new(vec![FieldSpec::new("email", FieldKind::Email)]),
            count: 10,
            seed: 1,
            locale: Locale::En,
        }],
    }
}

fn exported_at() -> chrono::DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 24, 10, 30, 0).unwrap()
}

/// Compares ignoring generated IDs.
fn same(a: &ProjectBundle, b: &ProjectBundle) {
    assert_eq!(a.project, b.project);
    assert_eq!(a.environments, b.environments);
    assert_eq!(a.datasets, b.datasets);
    assert_eq!(a.endpoints.len(), b.endpoints.len());
    for (x, y) in a.endpoints.iter().zip(&b.endpoints) {
        assert_eq!(
            Endpoint {
                id: y.id,
                ..x.clone()
            },
            *y
        );
    }
    assert_eq!(a.resources.len(), b.resources.len());
    for (x, y) in a.resources.iter().zip(&b.resources) {
        assert_eq!(
            Resource {
                id: y.resource.id,
                ..x.resource.clone()
            },
            y.resource
        );
        assert_eq!(x.records, y.records);
    }
}

fn file_names(bytes: &[u8]) -> Vec<String> {
    let zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    zip.file_names().map(|n| n.unwrap().into_owned()).collect()
}

#[test]
fn round_trip() {
    let original = bundle();
    let bytes = write(&original, exported_at(), "developer-workbench test").unwrap();

    let mut names = file_names(&bytes);
    names.sort();
    assert_eq!(
        names,
        [
            "workbench-project/datasets/01-customers.json",
            "workbench-project/endpoints/001-list-users.json",
            "workbench-project/endpoints/002-get-user-by-id.json",
            "workbench-project/environments/01-development.json",
            "workbench-project/environments/02-testing.json",
            "workbench-project/manifest.json",
            "workbench-project/openapi/openapi.yaml",
            "workbench-project/project.json",
            "workbench-project/resources/products.json",
            "workbench-project/resources/products.records.json",
            "workbench-project/resources/reviews.json",
        ]
    );

    let back = read(&bytes, &Limits::default()).unwrap();
    assert!(back.warnings.is_empty(), "{:?}", back.warnings);
    assert_eq!(back.manifest.format_version, FORMAT_VERSION);
    assert_eq!(back.manifest.project_name, "Shop");
    assert_eq!(back.manifest.contents.records, 2);
    assert!(!back.manifest.secrets_included);
    same(&original, &back.bundle);
    assert_ne!(
        back.bundle.endpoints[0].id, original.endpoints[0].id,
        "new IDs"
    );

    // Writing is deterministic.
    assert_eq!(
        bytes,
        write(&original, exported_at(), "developer-workbench test").unwrap()
    );
}

#[test]
fn files_carry_no_ids_or_secret_values() {
    let bytes = write(&bundle(), exported_at(), "test").unwrap();
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut endpoint = String::new();
    std::io::Read::read_to_string(
        &mut zip
            .by_name("workbench-project/endpoints/001-list-users.json")
            .unwrap(),
        &mut endpoint,
    )
    .unwrap();
    let endpoint: Value = serde_json::from_str(&endpoint).unwrap();
    assert!(endpoint.get("id").is_none());
    assert_eq!(endpoint["name"], "List users");
}

fn zip_of(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn manifest(version: u32) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "format": "workbench-project",
        "format_version": version,
        "generator": "test",
        "exported_at": "2026-01-01T00:00:00Z",
        "project_name": "P",
        "contents": { "environments": 0, "endpoints": 0, "resources": 0, "datasets": 0, "records": 0 }
    }))
    .unwrap()
}

const PROJECT: &[u8] = br#"{ "name": "P" }"#;

#[test]
fn accepts_a_flat_layout_and_reports_unknown_files() {
    let m = manifest(FORMAT_VERSION);
    let bytes = zip_of(&[
        ("manifest.json", &m),
        ("project.json", PROJECT),
        ("notes.txt", b"hello"),
        ("endpoints/nested/x.json", b"{}"),
    ]);
    let back = read(&bytes, &Limits::default()).unwrap();
    assert_eq!(back.bundle.project.name, "P");
    assert_eq!(back.warnings.len(), 2, "{:?}", back.warnings);
}

#[test]
fn rejects_unsafe_and_foreign_archives() {
    let m = manifest(FORMAT_VERSION);
    for evil in [
        "../evil.json",
        "/abs.json",
        "a\\b.json",
        "C:/x.json",
        "a/./b.json",
    ] {
        let bytes = zip_of(&[("manifest.json", &m), (evil, b"{}")]);
        assert!(
            matches!(
                read(&bytes, &Limits::default()),
                Err(ArchiveError::UnsafePath(_))
            ),
            "{evil}"
        );
    }
    assert!(matches!(
        read(b"not a zip", &Limits::default()),
        Err(ArchiveError::NotZip(_))
    ));
    assert!(matches!(
        read(&zip_of(&[("project.json", PROJECT)]), &Limits::default()),
        Err(ArchiveError::MissingManifest)
    ));
    let wrong = br#"{ "format": "something-else", "format_version": 1 }"#;
    assert!(matches!(
        read(&zip_of(&[("manifest.json", wrong)]), &Limits::default()),
        Err(ArchiveError::WrongFormat(_))
    ));
    let newer = manifest(FORMAT_VERSION + 1);
    assert!(matches!(
        read(
            &zip_of(&[("manifest.json", &newer), ("project.json", PROJECT)]),
            &Limits::default()
        ),
        Err(ArchiveError::NewerFormat { .. })
    ));
}

#[test]
fn enforces_size_limits() {
    let m = manifest(FORMAT_VERSION);
    // Highly compressible: tiny in the zip, large when expanded.
    let big = vec![b' '; 2 * 1024 * 1024];
    let bytes = zip_of(&[
        ("manifest.json", &m),
        ("project.json", PROJECT),
        ("endpoints/1.json", &big),
    ]);
    assert!(bytes.len() < 100 * 1024);
    let small = Limits {
        max_entry_bytes: 1024 * 1024,
        ..Limits::default()
    };
    assert!(matches!(
        read(&bytes, &small),
        Err(ArchiveError::EntryTooLarge { .. })
    ));
    let total = Limits {
        max_total_bytes: 1024 * 1024,
        ..Limits::default()
    };
    assert!(matches!(
        read(&bytes, &total),
        Err(ArchiveError::TooLarge { .. })
    ));
    let entries = Limits {
        max_entries: 2,
        ..Limits::default()
    };
    assert!(matches!(
        read(&bytes, &entries),
        Err(ArchiveError::TooManyEntries { .. })
    ));
}

#[test]
fn validates_every_item() {
    let mut b = bundle();
    b.endpoints[0].name = " ".into();
    b.resources[1].resource.name = "products".into();
    b.resources[0].records = Some(vec![json!({ "id": "a" }), json!({ "id": "a" }), json!({})]);
    b.datasets[0].count = 0;
    let bytes = write(&b, exported_at(), "test").unwrap();
    let Err(ArchiveError::Invalid(problems)) = read(&bytes, &Limits::default()) else {
        panic!("expected validation errors");
    };
    let text = problems.join("\n");
    assert!(text.contains("endpoints/001"), "{text}");
    assert!(text.contains("repeats key `a`"), "{text}");
    assert!(text.contains("record 2 has no `id` key"), "{text}");
    assert!(text.contains("datasets/01-customers.json"), "{text}");
}

#[test]
fn network_exposure_is_reset_on_import() {
    let mut b = bundle();
    b.environments[0].bind = BindAddress {
        host: IpAddr::V4(Ipv4Addr::UNSPECIFIED),
        port: 4010,
        allow_network_exposure: true,
    };
    let bytes = write(&b, exported_at(), "test").unwrap();
    let back = read(&bytes, &Limits::default()).unwrap();
    assert_eq!(
        back.bundle.environments[0].bind,
        BindAddress::loopback(4010)
    );
    assert!(back.warnings[0].contains("reset to loopback"));
}

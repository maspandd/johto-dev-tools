//! Writing archives.

use std::io::{Cursor, Write};

use chrono::{DateTime, Datelike, Timelike, Utc};
use serde::Serialize;
use serde_json::Value;
use zip::CompressionMethod;
use zip::write::SimpleFileOptions;

use crate::{ArchiveError, FORMAT, FORMAT_VERSION, Manifest, ProjectBundle, ROOT_DIR, slug};

/// Serializes `value` without its `id` (archives never carry IDs).
fn without_id<T: Serialize>(value: &T) -> Result<Value, ArchiveError> {
    let mut v = serde_json::to_value(value).map_err(|e| ArchiveError::Write(e.to_string()))?;
    if let Value::Object(map) = &mut v {
        map.shift_remove("id");
    }
    Ok(v)
}

fn pretty<T: Serialize>(value: &T) -> Result<Vec<u8>, ArchiveError> {
    let mut bytes =
        serde_json::to_vec_pretty(value).map_err(|e| ArchiveError::Write(e.to_string()))?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn numbered(index: usize, total: usize, name: &str) -> String {
    let width = total.to_string().len().max(2);
    format!("{:0width$}-{}", index + 1, slug(name))
}

/// Builds a zip archive. `generator` names the app, e.g.
/// `developer-workbench 0.1.0`.
pub fn write(
    bundle: &ProjectBundle,
    exported_at: DateTime<Utc>,
    generator: &str,
) -> Result<Vec<u8>, ArchiveError> {
    let manifest = Manifest {
        format: FORMAT.to_owned(),
        format_version: FORMAT_VERSION,
        generator: generator.to_owned(),
        exported_at,
        project_name: bundle.project.name.clone(),
        contents: bundle.contents(),
        secrets_included: false,
    };

    let mut files: Vec<(String, Vec<u8>)> = vec![
        ("manifest.json".into(), pretty(&manifest)?),
        ("project.json".into(), pretty(&bundle.project)?),
    ];
    let n = bundle.environments.len();
    for (i, env) in bundle.environments.iter().enumerate() {
        files.push((
            format!("environments/{}.json", numbered(i, n, &env.name)),
            pretty(env)?,
        ));
    }
    let n = bundle.endpoints.len();
    for (i, ep) in bundle.endpoints.iter().enumerate() {
        files.push((
            format!("endpoints/{}.json", numbered(i, n.max(100), &ep.name)),
            pretty(&without_id(ep)?)?,
        ));
    }
    let mut used = std::collections::HashSet::new();
    for entry in &bundle.resources {
        let mut name = slug(&entry.resource.name);
        let mut n = 2;
        while !used.insert(name.clone()) {
            name = format!("{}-{n}", slug(&entry.resource.name));
            n += 1;
        }
        files.push((
            format!("resources/{name}.json"),
            pretty(&without_id(&entry.resource)?)?,
        ));
        if let Some(records) = &entry.records {
            files.push((format!("resources/{name}.records.json"), pretty(records)?));
        }
    }
    let n = bundle.datasets.len();
    for (i, d) in bundle.datasets.iter().enumerate() {
        files.push((
            format!("datasets/{}.json", numbered(i, n, &d.name)),
            pretty(d)?,
        ));
    }
    let resources: Vec<_> = bundle
        .resources
        .iter()
        .map(|r| r.resource.clone())
        .collect();
    let openapi = openapi_engine::export(&openapi_engine::ApiDefinition {
        title: &bundle.project.name,
        description: &bundle.project.description,
        version: "1.0.0",
        base_path: bundle.project.base_path.as_deref(),
        endpoints: &bundle.endpoints,
        resources: &resources,
    });
    files.push((
        "openapi/openapi.yaml".into(),
        openapi_engine::to_yaml(&openapi).into_bytes(),
    ));

    let time = zip::DateTime::from_date_and_time(
        u16::try_from(exported_at.year()).unwrap_or(1980),
        u8::try_from(exported_at.month()).unwrap_or(1),
        u8::try_from(exported_at.day()).unwrap_or(1),
        u8::try_from(exported_at.hour()).unwrap_or(0),
        u8::try_from(exported_at.minute()).unwrap_or(0),
        u8::try_from(exported_at.second()).unwrap_or(0),
    )
    .unwrap_or_default();
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .last_modified_time(time)
        .unix_permissions(0o644);

    let err = |e: &dyn std::fmt::Display| ArchiveError::Write(e.to_string());
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in files {
        zip.start_file(format!("{ROOT_DIR}/{name}"), options)
            .map_err(|e| err(&e))?;
        zip.write_all(&bytes).map_err(|e| err(&e))?;
    }
    Ok(zip.finish().map_err(|e| err(&e))?.into_inner())
}

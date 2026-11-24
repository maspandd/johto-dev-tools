//! Reading archives safely.

use std::collections::{BTreeMap, HashSet};
use std::io::{Cursor, Read};

use serde::de::DeserializeOwned;
use serde_json::Value;
use workbench_core::{BindAddress, EndpointId, ProjectId, ResourceId, WorkspaceId};
use workbench_domain::{
    Dataset, Endpoint, Environment, Project, Resource, find_route_conflicts, key_of,
};
use zip::CompressionMethod;

use crate::migrate::{Files, migrate};
use crate::{
    ArchiveError, DatasetFile, EnvironmentFile, FORMAT, Manifest, ProjectBundle, ProjectFile,
    ResourceEntry,
};

/// Caps applied while reading. Sizes are uncompressed bytes.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_archive_bytes: usize,
    pub max_entries: usize,
    pub max_entry_bytes: u64,
    pub max_total_bytes: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_archive_bytes: 20 * 1024 * 1024,
            max_entries: 5_000,
            max_entry_bytes: 16 * 1024 * 1024,
            max_total_bytes: 64 * 1024 * 1024,
        }
    }
}

/// A validated archive.
#[derive(Debug, Clone)]
pub struct ReadArchive {
    pub manifest: Manifest,
    pub bundle: ProjectBundle,
    /// Things that were ignored or adjusted.
    pub warnings: Vec<String>,
}

fn is_safe_name(name: &str) -> bool {
    !name.is_empty()
        && !name.starts_with('/')
        && !name.contains(['\\', '\0', ':'])
        && name
            .trim_end_matches('/')
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

/// Reads and validates an archive. Nothing is written to disk.
pub fn read(bytes: &[u8], limits: &Limits) -> Result<ReadArchive, ArchiveError> {
    if bytes.len() > limits.max_archive_bytes {
        return Err(ArchiveError::TooLarge {
            max: limits.max_archive_bytes as u64,
        });
    }
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|e| ArchiveError::NotZip(e.to_string()))?;
    if zip.len() > limits.max_entries {
        return Err(ArchiveError::TooManyEntries {
            max: limits.max_entries,
        });
    }

    // The project folder is wherever manifest.json is: at the top, or one
    // folder down (normally `workbench-project/`).
    let names: Vec<String> = zip
        .file_names()
        .map(|n| n.map(std::borrow::Cow::into_owned))
        .collect::<Result<_, _>>()
        .map_err(|e| ArchiveError::NotZip(e.to_string()))?;
    for name in &names {
        if !is_safe_name(name) {
            return Err(ArchiveError::UnsafePath(name.clone()));
        }
    }
    let root = names
        .iter()
        .filter_map(|n| n.strip_suffix("manifest.json"))
        .filter(|prefix| {
            prefix.is_empty() || (prefix.matches('/').count() == 1 && prefix.ends_with('/'))
        })
        .min_by_key(|prefix| prefix.len())
        .ok_or(ArchiveError::MissingManifest)?
        .to_owned();

    let mut warnings = Vec::new();
    let mut files = Files::new();
    let mut total: u64 = 0;
    for i in 0..zip.len() {
        let (name, is_dir, is_symlink) = {
            let raw = zip
                .by_index_raw(i)
                .map_err(|e| ArchiveError::NotZip(e.to_string()))?;
            let name = raw
                .name()
                .map_err(|e| ArchiveError::NotZip(e.to_string()))?
                .into_owned();
            if raw.encrypted() {
                return Err(ArchiveError::Encrypted(name));
            }
            if !matches!(
                raw.compression(),
                CompressionMethod::Stored | CompressionMethod::Deflated
            ) {
                return Err(ArchiveError::UnsupportedCompression { name });
            }
            if raw.size() > limits.max_entry_bytes {
                return Err(ArchiveError::EntryTooLarge {
                    name,
                    max: limits.max_entry_bytes,
                });
            }
            let symlink = raw.unix_mode().is_some_and(|m| m & 0o170_000 == 0o120_000);
            (name, raw.is_dir(), symlink)
        };
        if is_dir {
            continue;
        }
        let Some(rel) = name.strip_prefix(&root) else {
            warnings.push(format!(
                "`{name}` is outside the project folder and was ignored"
            ));
            continue;
        };
        if is_symlink {
            warnings.push(format!("symbolic link `{name}` was ignored"));
            continue;
        }
        if rel.starts_with("openapi/") {
            continue;
        }
        let known = matches!(rel, "manifest.json" | "project.json")
            || ["environments/", "endpoints/", "resources/", "datasets/"]
                .iter()
                .any(|dir| {
                    rel.strip_prefix(dir)
                        .is_some_and(|f| !f.contains('/') && f.ends_with(".json"))
                });
        if !known {
            warnings.push(format!("unknown file `{rel}` was ignored"));
            continue;
        }

        let file = zip
            .by_index(i)
            .map_err(|e| ArchiveError::NotZip(e.to_string()))?;
        let mut data = Vec::new();
        file.take(limits.max_entry_bytes + 1)
            .read_to_end(&mut data)
            .map_err(|e| ArchiveError::InvalidFile {
                file: rel.to_owned(),
                message: e.to_string(),
            })?;
        let len = data.len() as u64;
        if len > limits.max_entry_bytes {
            return Err(ArchiveError::EntryTooLarge {
                name,
                max: limits.max_entry_bytes,
            });
        }
        total += len;
        if total > limits.max_total_bytes {
            return Err(ArchiveError::TooLarge {
                max: limits.max_total_bytes,
            });
        }
        let value: Value =
            serde_json::from_slice(&data).map_err(|e| ArchiveError::InvalidFile {
                file: rel.to_owned(),
                message: format!("not valid JSON: {e}"),
            })?;
        files.insert(rel.to_owned(), value);
    }

    let manifest_value = files
        .remove("manifest.json")
        .ok_or(ArchiveError::MissingManifest)?;
    let format = manifest_value
        .get("format")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if format != FORMAT {
        return Err(ArchiveError::WrongFormat(format.to_owned()));
    }
    let version = manifest_value
        .get("format_version")
        .and_then(Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0);
    migrate(version, &mut files)?;
    let manifest: Manifest = typed("manifest.json", manifest_value)?;

    let bundle = assemble(files, &mut warnings)?;
    let actual = bundle.contents();
    if actual != manifest.contents {
        warnings.push(
            "the manifest's item counts do not match the archive contents; the contents were used"
                .to_owned(),
        );
    }
    Ok(ReadArchive {
        manifest,
        bundle,
        warnings,
    })
}

fn typed<T: DeserializeOwned>(file: &str, value: Value) -> Result<T, ArchiveError> {
    serde_json::from_value(value).map_err(|e| ArchiveError::InvalidFile {
        file: file.to_owned(),
        message: e.to_string(),
    })
}

fn take_dir(files: &mut Files, dir: &str) -> BTreeMap<String, Value> {
    let keys: Vec<String> = files
        .keys()
        .filter(|k| k.starts_with(dir))
        .cloned()
        .collect();
    keys.into_iter()
        .filter_map(|k| files.remove(&k).map(|v| (k, v)))
        .collect()
}

fn assemble(mut files: Files, warnings: &mut Vec<String>) -> Result<ProjectBundle, ArchiveError> {
    let mut problems = Vec::new();
    let project: ProjectFile = typed(
        "project.json",
        files
            .remove("project.json")
            .ok_or(ArchiveError::InvalidFile {
                file: "project.json".into(),
                message: "missing".into(),
            })?,
    )?;
    let mut check = Project::new(WorkspaceId::new(), project.name.clone());
    check.description.clone_from(&project.description);
    check.base_path.clone_from(&project.base_path);
    check.default_headers.clone_from(&project.default_headers);
    if let Err(errors) = check.validate() {
        problems.extend(errors.iter().map(|e| format!("project.json: {e}")));
    }

    let mut environments = Vec::new();
    let mut names = HashSet::new();
    for (file, value) in take_dir(&mut files, "environments/") {
        let mut env: EnvironmentFile = typed(&file, value)?;
        if !env.bind.host.is_loopback() || env.bind.allow_network_exposure {
            warnings.push(format!(
                "environment `{}` listened on {}; it was reset to loopback (allow network exposure again deliberately)",
                env.name, env.bind.host
            ));
            env.bind = BindAddress::loopback(env.bind.port);
        }
        let mut check = Environment::new(ProjectId::new(), env.name.clone());
        check.base_url.clone_from(&env.base_url);
        check.default_headers.clone_from(&env.default_headers);
        check.variables.clone_from(&env.variables);
        check.bind = env.bind;
        if let Err(errors) = check.validate() {
            problems.extend(errors.iter().map(|e| format!("{file}: {e}")));
        }
        if !names.insert(env.name.to_lowercase()) {
            problems.push(format!(
                "{file}: another environment is named `{}`",
                env.name
            ));
        }
        environments.push(env);
    }

    let mut endpoints = Vec::new();
    for (file, value) in take_dir(&mut files, "endpoints/") {
        let mut ep: Endpoint = typed(&file, value)?;
        ep.id = EndpointId::new();
        if let Err(errors) = ep.validate() {
            problems.extend(errors.iter().map(|e| format!("{file}: {e}")));
        }
        endpoints.push(ep);
    }
    for conflict in find_route_conflicts(endpoints.iter().filter(|e| e.enabled)) {
        warnings.push(format!(
            "{} enabled endpoints answer {} {}; only one can be enabled when a server starts",
            conflict.endpoints.len(),
            conflict.method,
            conflict.shape
        ));
    }

    let mut resource_files = take_dir(&mut files, "resources/");
    let mut record_files = BTreeMap::new();
    resource_files.retain(|k, v| {
        if let Some(stem) = k.strip_suffix(".records.json") {
            record_files.insert(stem.to_owned(), std::mem::take(v));
            false
        } else {
            true
        }
    });
    let mut resources = Vec::new();
    let mut names = HashSet::new();
    for (file, value) in resource_files {
        let mut resource: Resource = typed(&file, value)?;
        resource.id = ResourceId::new();
        if let Err(errors) = resource.validate() {
            problems.extend(errors.iter().map(|e| format!("{file}: {e}")));
        }
        if !names.insert(resource.name.clone()) {
            problems.push(format!(
                "{file}: another resource is named `{}`",
                resource.name
            ));
        }
        let stem = file.trim_end_matches(".json");
        let records = record_files
            .remove(stem)
            .map(|v| records(&format!("{stem}.records.json"), v, &resource, &mut problems));
        resources.push(ResourceEntry {
            resource,
            records: records.flatten(),
        });
    }
    for stem in record_files.keys() {
        warnings.push(format!(
            "`{stem}.records.json` has no matching resource and was ignored"
        ));
    }

    let mut datasets = Vec::new();
    let mut names = HashSet::new();
    for (file, value) in take_dir(&mut files, "datasets/") {
        let d: DatasetFile = typed(&file, value)?;
        let mut check = Dataset::new(ProjectId::new(), d.name.clone(), d.schema.clone(), d.count);
        check.seed = d.seed;
        check.locale = d.locale;
        if let Err(errors) = check.validate() {
            problems.extend(errors.iter().map(|e| format!("{file}: {e}")));
        }
        if !names.insert(d.name.to_lowercase()) {
            problems.push(format!("{file}: another dataset is named `{}`", d.name));
        }
        datasets.push(d);
    }

    if !problems.is_empty() {
        return Err(ArchiveError::Invalid(problems));
    }
    Ok(ProjectBundle {
        project,
        environments,
        endpoints,
        resources,
        datasets,
    })
}

fn records(
    file: &str,
    value: Value,
    resource: &Resource,
    problems: &mut Vec<String>,
) -> Option<Vec<Value>> {
    let Value::Array(items) = value else {
        problems.push(format!("{file}: must be a JSON array of records"));
        return None;
    };
    let mut keys = HashSet::new();
    for (i, record) in items.iter().enumerate() {
        match record.get(&resource.primary_key).and_then(key_of) {
            Some(key) => {
                if !keys.insert(key.clone()) {
                    problems.push(format!("{file}: record {i} repeats key `{key}`"));
                }
            }
            None => problems.push(format!(
                "{file}: record {i} has no `{}` key",
                resource.primary_key
            )),
        }
    }
    Some(items)
}

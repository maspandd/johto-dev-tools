//! Field-by-field record generation from a single seeded ChaCha stream.

use std::collections::{HashMap, HashSet};

use chrono::{Duration, SecondsFormat};
use rand::seq::IndexedRandom;
use rand::{Rng, RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;
use serde_json::{Map, Number, Value};
use workbench_domain::{DataSchema, FieldKind, FieldSpec, Locale};

use crate::words::{COMPANY_SUFFIXES, EMAIL_DOMAINS, PLAIN_WORDS, STREET_SUFFIXES, Words, words};
use crate::{GenerateError, References};

/// Attempts at a fresh value before a unique field gives up.
const UNIQUE_ATTEMPTS: usize = 1_000;

pub(crate) struct Generator<'r> {
    rng: ChaCha8Rng,
    locale: Locale,
    refs: &'r References<'r>,
    seen: HashMap<String, HashSet<String>>,
}

fn pick<'a>(rng: &mut ChaCha8Rng, list: &'a [&'a str]) -> &'a str {
    list.choose(rng).copied().unwrap_or_default()
}

fn ascii_lower(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

impl<'r> Generator<'r> {
    pub(crate) fn new(seed: u64, locale: Locale, refs: &'r References<'r>) -> Self {
        Self {
            rng: ChaCha8Rng::seed_from_u64(seed),
            locale,
            refs,
            seen: HashMap::new(),
        }
    }

    const fn words(&self) -> &'static Words {
        words(self.locale)
    }

    pub(crate) fn record(
        &mut self,
        schema: &DataSchema,
        index: usize,
    ) -> Result<Value, GenerateError> {
        self.object(&schema.fields, index, "").map(Value::Object)
    }

    fn object(
        &mut self,
        fields: &[FieldSpec],
        index: usize,
        at: &str,
    ) -> Result<Map<String, Value>, GenerateError> {
        let mut map = Map::new();
        for f in fields {
            let path = if at.is_empty() {
                f.name.clone()
            } else {
                format!("{at}.{}", f.name)
            };
            let value = self.field(f, index, &map, &path)?;
            map.insert(f.name.clone(), value);
        }
        Ok(map)
    }

    fn field(
        &mut self,
        f: &FieldSpec,
        index: usize,
        siblings: &Map<String, Value>,
        path: &str,
    ) -> Result<Value, GenerateError> {
        if f.null_rate > 0.0 && self.rng.random_bool(f.null_rate) {
            return Ok(Value::Null);
        }
        if !f.unique {
            return self.value(&f.kind, index, siblings, path);
        }
        for _ in 0..UNIQUE_ATTEMPTS {
            let v = self.value(&f.kind, index, siblings, path)?;
            if v.is_null()
                || self
                    .seen
                    .entry(path.to_owned())
                    .or_default()
                    .insert(v.to_string())
            {
                return Ok(v);
            }
        }
        Err(GenerateError::UniqueExhausted {
            field: path.to_owned(),
        })
    }

    fn digits(&mut self, n: usize) -> String {
        (0..n)
            .map(|_| char::from(b'0' + self.rng.random_range(0..10u8)))
            .collect()
    }

    fn value(
        &mut self,
        kind: &FieldKind,
        index: usize,
        siblings: &Map<String, Value>,
        path: &str,
    ) -> Result<Value, GenerateError> {
        let w = self.words();
        let v = match kind {
            FieldKind::Uuid => {
                let mut bytes = [0u8; 16];
                self.rng.fill_bytes(&mut bytes);
                Value::String(
                    uuid::Builder::from_random_bytes(bytes)
                        .into_uuid()
                        .to_string(),
                )
            }
            FieldKind::Sequence { start, step } => {
                let i = i64::try_from(index).unwrap_or(i64::MAX);
                Value::from(start.saturating_add(step.saturating_mul(i)))
            }
            FieldKind::Integer { min, max } => Value::from(self.rng.random_range(*min..=*max)),
            FieldKind::Decimal { min, max, scale } => {
                let raw = if min == max {
                    *min
                } else {
                    self.rng.random_range(*min..=*max)
                };
                let factor = 10f64.powi(i32::try_from(*scale).unwrap_or(10));
                Number::from_f64((raw * factor).round() / factor).map_or(Value::Null, Value::Number)
            }
            FieldKind::Boolean => Value::Bool(self.rng.random_bool(0.5)),
            FieldKind::FirstName => Value::from(pick(&mut self.rng, w.first_names)),
            FieldKind::LastName => Value::from(pick(&mut self.rng, w.last_names)),
            FieldKind::FullName => {
                let first = pick(&mut self.rng, w.first_names);
                let last = pick(&mut self.rng, w.last_names);
                Value::from(format!("{first} {last}"))
            }
            FieldKind::Email => {
                let first = ascii_lower(pick(&mut self.rng, w.first_names));
                let last = ascii_lower(pick(&mut self.rng, w.last_names));
                let n = self.rng.random_range(1..1000u32);
                let domain = pick(&mut self.rng, EMAIL_DOMAINS);
                Value::from(format!("{first}.{last}{n}@{domain}"))
            }
            FieldKind::Phone => Value::from(match self.locale {
                Locale::En => format!("+1-555-{}-{}", self.digits(3), self.digits(4)),
                Locale::Id => format!(
                    "+62-8{}-{}-{}",
                    self.digits(2),
                    self.digits(4),
                    self.digits(4)
                ),
            }),
            FieldKind::Address => {
                let n = self.rng.random_range(1..=9999u32);
                let street = pick(&mut self.rng, w.streets);
                let city = pick(&mut self.rng, w.cities);
                Value::from(match self.locale {
                    Locale::En => {
                        let suffix = pick(&mut self.rng, STREET_SUFFIXES);
                        format!("{n} {street} {suffix}, {city}")
                    }
                    Locale::Id => format!("Jl. {street} No. {}, {city}", n % 300 + 1),
                })
            }
            FieldKind::City => Value::from(pick(&mut self.rng, w.cities)),
            FieldKind::Company => {
                let a = pick(&mut self.rng, w.company_a);
                let b = pick(&mut self.rng, w.company_b);
                Value::from(match self.locale {
                    Locale::En => format!("{a} {b} {}", pick(&mut self.rng, COMPANY_SUFFIXES)),
                    Locale::Id => format!("PT {a} {b}"),
                })
            }
            FieldKind::Words { min, max } => {
                let n = self.rng.random_range(*min..=*max);
                let text: Vec<&str> = (0..n).map(|_| pick(&mut self.rng, PLAIN_WORDS)).collect();
                Value::from(text.join(" "))
            }
            FieldKind::Date { from, to } => {
                let days = (*to - *from).num_days();
                let d = *from + Duration::days(self.rng.random_range(0..=days));
                Value::from(d.format("%Y-%m-%d").to_string())
            }
            FieldKind::Timestamp { from, to } => {
                let secs = (*to - *from).num_seconds();
                let t = *from + Duration::seconds(self.rng.random_range(0..=secs));
                Value::from(t.to_rfc3339_opts(SecondsFormat::Secs, true))
            }
            FieldKind::Enum { values } => {
                values.choose(&mut self.rng).cloned().unwrap_or(Value::Null)
            }
            FieldKind::Constant { value } => value.clone(),
            FieldKind::Template { template } => Value::from(self.template(template, siblings)),
            FieldKind::Object { fields } => Value::Object(self.object(fields, index, path)?),
            FieldKind::Array {
                items,
                min_items,
                max_items,
            } => {
                let n = self.rng.random_range(*min_items..=*max_items);
                let item_path = format!("{path}[]");
                let list = (0..n)
                    .map(|_| self.value(items, index, siblings, &item_path))
                    .collect::<Result<Vec<_>, _>>()?;
                Value::Array(list)
            }
            FieldKind::Reference { resource, field } => {
                let records =
                    self.refs
                        .get(resource)
                        .ok_or_else(|| GenerateError::UnknownReference {
                            field: path.to_owned(),
                            resource: resource.clone(),
                        })?;
                let record =
                    records
                        .choose(&mut self.rng)
                        .ok_or_else(|| GenerateError::EmptyReference {
                            field: path.to_owned(),
                            resource: resource.clone(),
                        })?;
                record.get(field).cloned().unwrap_or(Value::Null)
            }
        };
        Ok(v)
    }

    fn template(&mut self, template: &str, siblings: &Map<String, Value>) -> String {
        let mut out = String::with_capacity(template.len());
        let mut rest = template;
        while let Some(c) = rest.chars().next() {
            if let Some(after) = rest.strip_prefix("{{")
                && let Some(end) = after.find("}}")
            {
                match siblings.get(after[..end].trim()) {
                    Some(Value::String(s)) => out.push_str(s),
                    Some(Value::Null) | None => {}
                    Some(other) => out.push_str(&other.to_string()),
                }
                rest = &after[end + 2..];
                continue;
            }
            match c {
                '#' => out.push(char::from(b'0' + self.rng.random_range(0..10u8))),
                '?' => out.push(char::from(b'A' + self.rng.random_range(0..26u8))),
                other => out.push(other),
            }
            rest = &rest[c.len_utf8()..];
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use crate::{GenerateError, PREVIEW_LIMIT, References, generate, preview};
    use chrono::{NaiveDate, TimeZone, Utc};
    use serde_json::{Value, json};
    use workbench_domain::{DataSchema, FieldKind, FieldSpec, Locale};

    fn f(name: &str, kind: FieldKind) -> FieldSpec {
        FieldSpec::new(name, kind)
    }

    fn user_schema() -> DataSchema {
        DataSchema::new(vec![
            f("id", FieldKind::Uuid),
            f(
                "seq",
                FieldKind::Sequence {
                    start: 100,
                    step: 10,
                },
            ),
            f("name", FieldKind::FullName),
            f("email", FieldKind::Email),
            f("phone", FieldKind::Phone),
            f("age", FieldKind::Integer { min: 18, max: 90 }),
            f(
                "score",
                FieldKind::Decimal {
                    min: 0.0,
                    max: 5.0,
                    scale: 1,
                },
            ),
            f("active", FieldKind::Boolean),
            f("company", FieldKind::Company),
            f("address", FieldKind::Address),
            f(
                "joined",
                FieldKind::Date {
                    from: NaiveDate::from_ymd_opt(2024, 1, 1).unwrap(),
                    to: NaiveDate::from_ymd_opt(2024, 12, 31).unwrap(),
                },
            ),
            f(
                "last_seen",
                FieldKind::Timestamp {
                    from: Utc.with_ymd_and_hms(2026, 1, 1, 0, 0, 0).unwrap(),
                    to: Utc.with_ymd_and_hms(2026, 1, 2, 0, 0, 0).unwrap(),
                },
            ),
            f(
                "role",
                FieldKind::Enum {
                    values: vec![json!("admin"), json!("member")],
                },
            ),
            f("bio", FieldKind::Words { min: 2, max: 4 }),
        ])
    }

    #[test]
    fn same_seed_same_output_and_different_seed_differs() {
        let refs = References::new();
        let a = generate(&user_schema(), 50, 7, Locale::En, &refs).unwrap();
        let b = generate(&user_schema(), 50, 7, Locale::En, &refs).unwrap();
        let c = generate(&user_schema(), 50, 8, Locale::En, &refs).unwrap();
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn values_respect_their_constraints() {
        let records = generate(&user_schema(), 200, 1, Locale::En, &References::new()).unwrap();
        for (i, r) in records.iter().enumerate() {
            assert!(uuid::Uuid::parse_str(r["id"].as_str().unwrap()).is_ok());
            assert_eq!(r["seq"], json!(100 + 10 * i));
            assert!((18..=90).contains(&r["age"].as_i64().unwrap()));
            let score = r["score"].as_f64().unwrap();
            assert!((0.0..=5.0).contains(&score) && (score * 10.0).fract() == 0.0);
            assert!(r["email"].as_str().unwrap().contains("@example."));
            assert!(r["phone"].as_str().unwrap().starts_with("+1-555-"));
            assert!(r["joined"].as_str().unwrap().starts_with("2024-"));
            assert!(r["last_seen"].as_str().unwrap().starts_with("2026-01-0"));
            assert!(["admin", "member"].contains(&r["role"].as_str().unwrap()));
            let words = r["bio"].as_str().unwrap().split(' ').count();
            assert!((2..=4).contains(&words));
        }
    }

    #[test]
    fn preview_is_a_prefix_of_the_full_dataset() {
        let refs = References::new();
        let full = generate(&user_schema(), 100, 3, Locale::En, &refs).unwrap();
        let head = preview(&user_schema(), 100, 3, Locale::En, &refs).unwrap();
        assert_eq!(head.len(), PREVIEW_LIMIT);
        assert_eq!(head, full[..PREVIEW_LIMIT]);
    }

    #[test]
    fn indonesian_locale() {
        let schema = DataSchema::new(vec![
            f("phone", FieldKind::Phone),
            f("address", FieldKind::Address),
            f("company", FieldKind::Company),
        ]);
        for r in generate(&schema, 20, 2, Locale::Id, &References::new()).unwrap() {
            assert!(r["phone"].as_str().unwrap().starts_with("+62-8"));
            assert!(r["address"].as_str().unwrap().starts_with("Jl. "));
            assert!(r["company"].as_str().unwrap().starts_with("PT "));
        }
    }

    #[test]
    fn null_rate_and_unique() {
        let schema = DataSchema::new(vec![
            FieldSpec {
                null_rate: 1.0,
                ..f("always_null", FieldKind::Boolean)
            },
            FieldSpec {
                unique: true,
                ..f("code", FieldKind::Integer { min: 1, max: 50 })
            },
        ]);
        let records = generate(&schema, 50, 4, Locale::En, &References::new()).unwrap();
        assert!(records.iter().all(|r| r["always_null"].is_null()));
        let mut codes: Vec<i64> = records
            .iter()
            .map(|r| r["code"].as_i64().unwrap())
            .collect();
        codes.sort_unstable();
        assert_eq!(codes, (1..=50).collect::<Vec<_>>());

        let err = generate(&schema, 51, 4, Locale::En, &References::new()).unwrap_err();
        assert_eq!(
            err,
            GenerateError::UniqueExhausted {
                field: "code".into()
            }
        );
    }

    #[test]
    fn nested_objects_arrays_and_templates() {
        let schema = DataSchema::new(vec![
            f("first", FieldKind::FirstName),
            f(
                "handle",
                FieldKind::Template {
                    template: "{{first}}-##-??".into(),
                },
            ),
            f(
                "address",
                FieldKind::Object {
                    fields: vec![
                        f("city", FieldKind::City),
                        f(
                            "zip",
                            FieldKind::Template {
                                template: "#####".into(),
                            },
                        ),
                    ],
                },
            ),
            f(
                "tags",
                FieldKind::Array {
                    items: Box::new(FieldKind::Words { min: 1, max: 1 }),
                    min_items: 1,
                    max_items: 3,
                },
            ),
            f(
                "kind",
                FieldKind::Constant {
                    value: json!({ "v": 1 }),
                },
            ),
        ]);
        for r in generate(&schema, 30, 5, Locale::En, &References::new()).unwrap() {
            let first = r["first"].as_str().unwrap();
            let handle = r["handle"].as_str().unwrap();
            let suffix = handle.strip_prefix(&format!("{first}-")).unwrap();
            assert!(suffix[..2].chars().all(|c| c.is_ascii_digit()));
            assert!(suffix[3..].chars().all(|c| c.is_ascii_uppercase()));
            assert_eq!(r["address"]["zip"].as_str().unwrap().len(), 5);
            assert!(r["address"]["city"].is_string());
            assert!((1..=3).contains(&r["tags"].as_array().unwrap().len()));
            assert_eq!(r["kind"], json!({ "v": 1 }));
        }
    }

    #[test]
    fn references_pick_existing_values() {
        let users = vec![json!({ "id": "u1" }), json!({ "id": "u2" })];
        let refs = References::new().with("users", &users);
        let schema = DataSchema::new(vec![f(
            "user_id",
            FieldKind::Reference {
                resource: "users".into(),
                field: "id".into(),
            },
        )]);
        let orders = generate(&schema, 20, 6, Locale::En, &refs).unwrap();
        assert!(
            orders
                .iter()
                .all(|o| ["u1", "u2"].contains(&o["user_id"].as_str().unwrap()))
        );

        let empty: Vec<Value> = vec![];
        let err = generate(
            &schema,
            1,
            6,
            Locale::En,
            &References::new().with("users", &empty),
        )
        .unwrap_err();
        assert!(matches!(err, GenerateError::EmptyReference { .. }));
        let err = generate(&schema, 1, 6, Locale::En, &References::new()).unwrap_err();
        assert!(matches!(err, GenerateError::UnknownReference { .. }));
    }

    #[test]
    fn limits_and_validation() {
        let schema = DataSchema::new(vec![f("n", FieldKind::Integer { min: 2, max: 1 })]);
        assert!(matches!(
            generate(&schema, 1, 0, Locale::En, &References::new()),
            Err(GenerateError::InvalidSchema(_))
        ));
        assert!(matches!(
            generate(
                &DataSchema::default(),
                10_001,
                0,
                Locale::En,
                &References::new()
            ),
            Err(GenerateError::TooMany { .. })
        ));
    }
}

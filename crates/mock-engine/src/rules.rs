//! Declarative response-rule evaluation.

use std::collections::BTreeMap;

use serde_json::Value;
use workbench_domain::{Condition, Operator, ResponseRule, ValueSource};

use crate::MockRequest;

pub(crate) struct RuleInput<'a> {
    pub request: &'a MockRequest,
    pub params: &'a BTreeMap<String, String>,
    pub body: Option<&'a Value>,
}

impl RuleInput<'_> {
    /// The condition's source value as text. JSON `null` counts as absent.
    fn read(&self, source: &ValueSource) -> Option<String> {
        match source {
            ValueSource::Header { name } => self.request.header(name).map(str::to_owned),
            ValueSource::Query { name } => self.request.query_param(name).map(str::to_owned),
            ValueSource::Path { name } => self.params.get(name).cloned(),
            ValueSource::Body { pointer } => match self.body?.pointer(pointer)? {
                Value::Null => None,
                Value::String(s) => Some(s.clone()),
                other => Some(other.to_string()),
            },
        }
    }

    fn holds(&self, cond: &Condition) -> bool {
        let actual = self.read(&cond.source);
        let expected = cond.value.as_deref().unwrap_or_default();
        match cond.operator {
            Operator::Equals => actual.as_deref() == Some(expected),
            Operator::NotEquals => actual.as_deref() != Some(expected),
            Operator::Contains => actual.is_some_and(|a| a.contains(expected)),
            Operator::Exists => actual.is_some(),
            Operator::Absent => actual.is_none(),
        }
    }

    /// First rule whose conditions all hold.
    pub(crate) fn first_match<'r>(&self, rules: &'r [ResponseRule]) -> Option<&'r ResponseRule> {
        rules.iter().find(|r| r.when.iter().all(|c| self.holds(c)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use workbench_domain::MockResponse;

    fn cond(source: ValueSource, operator: Operator, value: Option<&str>) -> Condition {
        Condition {
            source,
            operator,
            value: value.map(str::to_owned),
        }
    }

    fn rule(name: &str, when: Vec<Condition>) -> ResponseRule {
        ResponseRule {
            name: name.into(),
            when,
            response: MockResponse::default(),
        }
    }

    #[test]
    fn evaluates_operators_against_all_sources() {
        let req = MockRequest::new("GET", "/u/7?mode=slow").with_header("X-Role", "admin");
        let params = BTreeMap::from([("id".to_owned(), "7".to_owned())]);
        let body = json!({ "user": { "age": 30, "name": "Alex Morgan", "nick": null } });
        let input = RuleInput {
            request: &req,
            params: &params,
            body: Some(&body),
        };
        let h = |n: &str| ValueSource::Header { name: n.into() };
        let q = |n: &str| ValueSource::Query { name: n.into() };
        let b = |p: &str| ValueSource::Body { pointer: p.into() };

        assert!(input.holds(&cond(h("x-role"), Operator::Equals, Some("admin"))));
        assert!(input.holds(&cond(q("mode"), Operator::NotEquals, Some("fast"))));
        assert!(input.holds(&cond(q("missing"), Operator::NotEquals, Some("x"))));
        assert!(input.holds(&cond(
            ValueSource::Path { name: "id".into() },
            Operator::Equals,
            Some("7")
        )));
        assert!(input.holds(&cond(b("/user/age"), Operator::Equals, Some("30"))));
        assert!(input.holds(&cond(b("/user/name"), Operator::Contains, Some("Morgan"))));
        assert!(input.holds(&cond(b("/user/nick"), Operator::Absent, None)));
        assert!(input.holds(&cond(b("/user"), Operator::Exists, None)));
        assert!(!input.holds(&cond(b("/nope"), Operator::Contains, Some(""))));
    }

    #[test]
    fn first_matching_rule_wins_and_all_conditions_must_hold() {
        let req = MockRequest::new("GET", "/?a=1&b=2");
        let params = BTreeMap::new();
        let input = RuleInput {
            request: &req,
            params: &params,
            body: None,
        };
        let a1 = cond(
            ValueSource::Query { name: "a".into() },
            Operator::Equals,
            Some("1"),
        );
        let b3 = cond(
            ValueSource::Query { name: "b".into() },
            Operator::Equals,
            Some("3"),
        );
        let rules = vec![
            rule("both", vec![a1.clone(), b3]),
            rule("a-only", vec![a1.clone()]),
            rule("a-again", vec![a1]),
        ];
        assert_eq!(input.first_match(&rules).unwrap().name, "a-only");
    }
}

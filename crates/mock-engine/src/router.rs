//! Route matching with static-over-parameter precedence.
//!
//! For a given method, `/users/me` beats `/users/:id`. Conflicts (same
//! method and shape) are rejected before a table is built, so the most
//! specific match is always unique.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use workbench_domain::{Method, Segment};

#[derive(Debug)]
struct Route {
    method: Method,
    segments: Vec<Segment>,
    /// Index into the engine's endpoint list.
    target: usize,
}

#[derive(Debug, Default)]
pub(crate) struct RouteTable {
    routes: Vec<Route>,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Lookup {
    Found {
        target: usize,
        params: BTreeMap<String, String>,
    },
    MethodNotAllowed {
        allowed: Vec<Method>,
    },
    NotFound,
}

/// Static segments rank above parameters, compared left to right.
fn specificity(a: &[Segment], b: &[Segment]) -> Ordering {
    for (x, y) in a.iter().zip(b) {
        match (x, y) {
            (Segment::Static(_), Segment::Param(_)) => return Ordering::Greater,
            (Segment::Param(_), Segment::Static(_)) => return Ordering::Less,
            _ => {}
        }
    }
    Ordering::Equal
}

fn match_segments(route: &[Segment], path: &[String]) -> Option<BTreeMap<String, String>> {
    if route.len() != path.len() {
        return None;
    }
    let mut params = BTreeMap::new();
    for (seg, part) in route.iter().zip(path) {
        match seg {
            Segment::Static(s) if s == part => {}
            Segment::Static(_) => return None,
            Segment::Param(name) => {
                params.insert(name.clone(), part.clone());
            }
        }
    }
    Some(params)
}

impl RouteTable {
    pub(crate) fn insert(&mut self, method: Method, segments: Vec<Segment>, target: usize) {
        self.routes.push(Route {
            method,
            segments,
            target,
        });
    }

    /// `method` is `None` when the request used a method the MVP does not
    /// support (e.g. `HEAD`); a matching path then yields `MethodNotAllowed`.
    pub(crate) fn lookup(&self, method: Option<Method>, path: &[String]) -> Lookup {
        let mut best: Option<(&Route, BTreeMap<String, String>)> = None;
        let mut allowed = Vec::new();

        for route in &self.routes {
            let Some(params) = match_segments(&route.segments, path) else {
                continue;
            };
            if Some(route.method) != method {
                allowed.push(route.method);
                continue;
            }
            let better = match &best {
                None => true,
                Some((current, _)) => {
                    specificity(&route.segments, &current.segments) == Ordering::Greater
                }
            };
            if better {
                best = Some((route, params));
            }
        }

        match best {
            Some((route, params)) => Lookup::Found {
                target: route.target,
                params,
            },
            None if !allowed.is_empty() => {
                allowed.sort();
                allowed.dedup();
                Lookup::MethodNotAllowed { allowed }
            }
            None => Lookup::NotFound,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use workbench_domain::PathTemplate;

    fn table(routes: &[(Method, &str)]) -> RouteTable {
        let mut t = RouteTable::default();
        for (i, (m, p)) in routes.iter().enumerate() {
            t.insert(*m, PathTemplate::parse(p).unwrap().segments().to_vec(), i);
        }
        t
    }

    fn segs(p: &str) -> Vec<String> {
        crate::request::path_segments(p)
    }

    #[test]
    fn matches_static_route() {
        let t = table(&[(Method::Get, "/users")]);
        assert!(matches!(
            t.lookup(Some(Method::Get), &segs("/users")),
            Lookup::Found { target: 0, .. }
        ));
        assert_eq!(
            t.lookup(Some(Method::Get), &segs("/nope")),
            Lookup::NotFound
        );
    }

    #[test]
    fn extracts_params() {
        let t = table(&[(Method::Get, "/users/:id/posts/:postId")]);
        let Lookup::Found { params, .. } = t.lookup(Some(Method::Get), &segs("/users/42/posts/7"))
        else {
            panic!("expected match");
        };
        assert_eq!(params["id"], "42");
        assert_eq!(params["postId"], "7");
    }

    #[test]
    fn static_beats_param_regardless_of_order() {
        let t = table(&[(Method::Get, "/users/:id"), (Method::Get, "/users/me")]);
        assert!(matches!(
            t.lookup(Some(Method::Get), &segs("/users/me")),
            Lookup::Found { target: 1, .. }
        ));
        assert!(matches!(
            t.lookup(Some(Method::Get), &segs("/users/42")),
            Lookup::Found { target: 0, .. }
        ));
    }

    #[test]
    fn leftmost_static_segment_wins() {
        let t = table(&[(Method::Get, "/:a/x"), (Method::Get, "/x/:b")]);
        assert!(matches!(
            t.lookup(Some(Method::Get), &segs("/x/x")),
            Lookup::Found { target: 1, .. }
        ));
    }

    #[test]
    fn method_mismatch_reports_allowed_methods() {
        let t = table(&[
            (Method::Get, "/users/:id"),
            (Method::Delete, "/users/:id"),
            (Method::Patch, "/users/:id"),
        ]);
        assert_eq!(
            t.lookup(Some(Method::Post), &segs("/users/1")),
            Lookup::MethodNotAllowed {
                allowed: vec![Method::Get, Method::Patch, Method::Delete]
            }
        );
        assert!(matches!(
            t.lookup(None, &segs("/users/1")),
            Lookup::MethodNotAllowed { .. }
        ));
    }

    #[test]
    fn method_match_preferred_over_more_specific_path() {
        let t = table(&[(Method::Get, "/users/me"), (Method::Post, "/users/:id")]);
        assert!(matches!(
            t.lookup(Some(Method::Post), &segs("/users/me")),
            Lookup::Found { target: 1, .. }
        ));
    }
}

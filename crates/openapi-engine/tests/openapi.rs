use openapi_engine::{ApiDefinition, Severity, analyze, export, parse_document, to_yaml};
use serde_json::json;
use workbench_domain::{
    AuthSimulation, Condition, DataSchema, Endpoint, FieldKind, FieldSpec, Method, MockResponse,
    Operator, PathTemplate, Resource, ResponseBody, ResponseRule, ValueSource,
};

const PETSTORE_30: &str = r##"
openapi: 3.0.3
info:
  title: Petstore
  description: Sample pets
  version: 2.1.0
servers:
  - url: https://{env}.example.test/{version}
    variables:
      env: { default: api }
      version: { default: v2 }
security:
  - bearerAuth: []
paths:
  /pets:
    parameters:
      - $ref: '#/components/parameters/Tenant'
    get:
      operationId: listPets
      summary: List pets
      parameters:
        - name: limit
          in: query
          required: false
          description: Page size
          schema: { type: integer }
        - name: session
          in: cookie
          schema: { type: string }
      responses:
        200:
          description: OK
          headers:
            X-Rate-Limit:
              schema: { type: integer, example: 100 }
          content:
            application/json:
              schema:
                type: array
                items: { $ref: '#/components/schemas/Pet' }
        default:
          $ref: '#/components/responses/Error'
    post:
      summary: Create a pet
      security: []
      requestBody:
        required: true
        content:
          application/json:
            schema: { $ref: '#/components/schemas/NewPet' }
            examples:
              rex: { value: { name: Rex, tag: dog } }
      responses:
        '201':
          description: Created
          content:
            application/json:
              example: { id: 1, name: Rex }
        '400':
          $ref: '#/components/responses/Error'
  /pets/{pet-id}:
    get:
      summary: Get a pet
      security:
        - apiKey: []
      responses:
        '200':
          description: OK
          content:
            application/json:
              schema: { $ref: '#/components/schemas/Pet' }
        '404':
          description: Not found
    head:
      responses: { '200': { description: OK } }
    delete:
      responses:
        '204': { description: Deleted }
  /pets/{id}:
    get:
      summary: Duplicate shape
      responses: { '200': { description: OK } }
  /files/{name}.json:
    get:
      responses: { '200': { description: OK } }
  /report:
    get:
      responses:
        '200':
          description: CSV
          content:
            text/csv: { example: "a,b" }
  /upload:
    post:
      requestBody:
        content:
          multipart/form-data:
            schema: { type: object }
      responses:
        '200':
          description: OK
          content:
            text/plain: { example: done }
components:
  parameters:
    Tenant:
      name: X-Tenant
      in: header
      required: true
  responses:
    Error:
      description: Error
      content:
        application/json:
          schema:
            type: object
            properties:
              message: { type: string }
  securitySchemes:
    bearerAuth: { type: http, scheme: bearer }
    apiKey: { type: apiKey, in: header, name: X-API-Key }
  schemas:
    Pet:
      allOf:
        - $ref: '#/components/schemas/NewPet'
        - type: object
          required: [id]
          properties:
            id: { type: integer, format: int64 }
            born: { type: string, format: date-time }
            parent: { $ref: '#/components/schemas/Pet' }
    NewPet:
      type: object
      required: [name]
      properties:
        name: { type: string }
        tag: { type: string, nullable: true }
"##;

fn op<'a>(a: &'a openapi_engine::Analysis, key: &str) -> &'a openapi_engine::Operation {
    a.operations
        .iter()
        .find(|o| o.key == key)
        .unwrap_or_else(|| panic!("no operation {key}"))
}

#[test]
fn imports_a_30_document() {
    let doc = parse_document(PETSTORE_30).unwrap();
    let a = analyze(&doc).unwrap();
    assert_eq!(a.title, "Petstore");
    assert_eq!(a.version, "2.1.0");
    assert_eq!(a.base_path.as_deref(), Some("/v2"));

    let list = op(&a, "GET /pets").endpoint.as_ref().unwrap();
    assert_eq!(list.name, "List pets");
    assert_eq!(list.auth, AuthSimulation::Bearer, "global security applies");
    assert_eq!(list.request.headers[0].name, "X-Tenant");
    assert!(list.request.headers[0].required);
    assert_eq!(list.request.query.len(), 1, "cookie parameter skipped");
    assert_eq!(list.response.headers["X-Rate-Limit"], "100");
    let ResponseBody::Json(body) = &list.response.body else {
        panic!("json body expected")
    };
    assert_eq!(body[0]["name"], "string");
    assert_eq!(body[0]["born"], "{{ now }}");
    let list_issues = &op(&a, "GET /pets").issues;
    assert!(
        list_issues
            .iter()
            .any(|i| i.message.contains("cookie parameter `session`"))
    );
    assert!(
        list_issues
            .iter()
            .any(|i| i.severity == Severity::Info && i.message.contains("default"))
    );

    let create = op(&a, "POST /pets").endpoint.as_ref().unwrap();
    assert_eq!(
        create.auth,
        AuthSimulation::None,
        "`security: []` disables auth"
    );
    assert_eq!(create.response.status, 201);
    assert_eq!(
        create.response.body,
        ResponseBody::Json(json!({ "id": 1, "name": "Rex" }))
    );
    assert_eq!(
        create.request.body_example,
        Some(json!({ "name": "Rex", "tag": "dog" }))
    );
    let schema = create.request.body_schema.as_ref().unwrap();
    let v = jsonschema::validator_for(schema).unwrap();
    assert!(v.is_valid(&json!({ "name": "Rex", "tag": null })));
    assert!(!v.is_valid(&json!({ "tag": "dog" })));

    let get = op(&a, "GET /pets/{pet-id}");
    let ep = get.endpoint.as_ref().unwrap();
    assert_eq!(ep.path.to_string(), "/pets/:pet_id");
    assert_eq!(
        ep.auth,
        AuthSimulation::ApiKey {
            header: "X-API-Key".into()
        }
    );
    assert!(get.issues.iter().any(|i| i.message.contains("renamed")));

    let delete = op(&a, "DELETE /pets/{pet-id}").endpoint.as_ref().unwrap();
    assert_eq!(delete.response.status, 204);
    assert_eq!(delete.response.body, ResponseBody::Empty);
    assert_eq!(delete.name, "DELETE /pets/{pet-id}");

    assert!(op(&a, "HEAD /pets/{pet-id}").endpoint.is_none());
    let dup = op(&a, "GET /pets/{id}");
    assert!(dup.endpoint.is_none());
    assert!(dup.issues[0].message.contains("same requests"));
    let mixed = op(&a, "GET /files/{name}.json");
    assert!(mixed.endpoint.is_none());
    assert_eq!(mixed.issues[0].severity, Severity::Error);

    let report = op(&a, "GET /report").endpoint.as_ref().unwrap();
    assert_eq!(report.response.body, ResponseBody::Text("a,b".into()));
    assert_eq!(report.response.headers["content-type"], "text/csv");

    let upload = op(&a, "POST /upload");
    assert!(
        upload
            .endpoint
            .as_ref()
            .unwrap()
            .request
            .body_schema
            .is_none()
    );
    assert!(
        upload
            .issues
            .iter()
            .any(|i| i.message.contains("multipart/form-data"))
    );

    // Pet's schema is recursive and shared; the sample is bounded.
    assert!(a.has_errors());
    assert_eq!(a.endpoints(None).unwrap().len(), 6);
}

#[test]
fn selective_import() {
    let a = analyze(&parse_document(PETSTORE_30).unwrap()).unwrap();
    let chosen = a
        .endpoints(Some(&["listPets".into(), "post /pets".into()]))
        .unwrap();
    assert_eq!(chosen.len(), 2);
    assert_eq!(chosen[1].method, Method::Post);

    let err = a
        .endpoints(Some(&["GET /nope".into(), "GET /pets/{id}".into()]))
        .unwrap_err();
    assert_eq!(err.len(), 2);
    assert!(err[0].message.contains("no operation"));
    assert!(err[1].message.contains("cannot be imported"));
}

#[test]
fn rejects_non_openapi_3() {
    let swagger = json!({ "swagger": "2.0", "paths": {} });
    assert!(
        analyze(&swagger).unwrap_err()[0]
            .message
            .contains("Swagger 2.0")
    );
    assert!(analyze(&json!({ "openapi": "4.0.0" })).is_err());
    assert!(analyze(&json!({ "info": {} })).is_err());
    let a =
        analyze(&json!({ "openapi": "3.1.0", "info": { "title": "x", "version": "1" } })).unwrap();
    assert!(a.operations.is_empty());
    assert_eq!(a.issues[0].pointer, "/paths");
}

fn endpoint(name: &str, method: Method, path: &str) -> Endpoint {
    Endpoint::new(name, method, PathTemplate::parse(path).unwrap())
}

#[test]
fn round_trips_through_export() {
    let mut get = endpoint("Get user", Method::Get, "/users/:id");
    get.description = "One user".into();
    get.auth = AuthSimulation::Bearer;
    get.request.query.push(workbench_domain::ParamSpec {
        name: "fields".into(),
        required: false,
        description: "Projection".into(),
    });
    get.response = MockResponse::json(
        200,
        json!({ "id": "{{ path.id }}", "created": "{{ now }}" }),
    );
    get.response
        .headers
        .insert("X-Source".into(), "mock".into());
    get.response.delay_ms = 25;
    get.rules.push(ResponseRule {
        name: "Missing".into(),
        when: vec![Condition {
            source: ValueSource::Path { name: "id".into() },
            operator: Operator::Equals,
            value: Some("0".into()),
        }],
        response: MockResponse::json(404, json!({ "error": "not found" })),
    });
    get.rules.push(ResponseRule {
        name: "Teapot".into(),
        when: vec![Condition {
            source: ValueSource::Header {
                name: "x-tea".into(),
            },
            operator: Operator::Exists,
            value: None,
        }],
        response: MockResponse::json(200, json!({ "tea": true })),
    });

    let mut create = endpoint("Create user", Method::Post, "/users");
    create.auth = AuthSimulation::ApiKey {
        header: "X-Key".into(),
    };
    create.request.body_schema = Some(json!({
        "$ref": "#/$defs/User",
        "$defs": { "User": { "type": "object", "required": ["name"], "properties": {
            "name": { "type": "string" },
            "friends": { "type": "array", "items": { "$ref": "#/$defs/User" } }
        }}}
    }));
    create.response = MockResponse::json(201, json!({ "ok": true }));

    let mut delete = endpoint("Delete user", Method::Delete, "/users/:userId");
    delete.enabled = false;
    delete.response.status = 204;

    let mut text = endpoint("Health", Method::Get, "/health");
    text.response.body = ResponseBody::Text("ok".into());

    let mut products = Resource::new("products", PathTemplate::parse("/products").unwrap());
    products.schema = DataSchema::new(vec![
        FieldSpec::new("name", FieldKind::FullName),
        FieldSpec::new(
            "price",
            FieldKind::Decimal {
                min: 1.0,
                max: 9.0,
                scale: 2,
            },
        ),
    ]);
    let mut hidden = Resource::new("hidden", PathTemplate::parse("/hidden").unwrap());
    hidden.enabled = false;

    let endpoints = vec![get, create, delete, text];
    let resources = vec![products, hidden];
    let doc = export(&ApiDefinition {
        title: "Users",
        description: "",
        version: "",
        base_path: Some("/api"),
        endpoints: &endpoints,
        resources: &resources,
    });

    assert_eq!(doc["openapi"], "3.1.0");
    assert_eq!(doc["servers"][0]["url"], "/api");
    assert!(
        doc["paths"].get("/users/{userId}").is_none(),
        "same shape shares one path key"
    );
    let delete_op = &doc["paths"]["/users/{id}"]["delete"];
    assert_eq!(delete_op["x-workbench"]["path"], "/users/:userId");
    assert_eq!(delete_op["x-workbench"]["enabled"], false);
    let get_op = &doc["paths"]["/users/{id}"]["get"];
    assert_eq!(get_op["security"], json!([{ "bearerAuth": [] }]));
    assert!(get_op["responses"]["200"]["content"]["application/json"]["examples"].is_object());
    let user_ref =
        &doc["paths"]["/users"]["post"]["requestBody"]["content"]["application/json"]["schema"];
    assert_eq!(user_ref["$ref"], "#/components/schemas/User");
    assert_eq!(
        doc["components"]["schemas"]["User"]["properties"]["friends"]["items"]["$ref"],
        "#/components/schemas/User"
    );
    assert!(doc["paths"].get("/products/{id}").is_some());
    assert!(doc["paths"].get("/hidden").is_none());
    assert_eq!(
        doc["components"]["schemas"]["products"]["properties"]["id"]["format"],
        "uuid"
    );

    // Through YAML and back.
    let again = parse_document(&to_yaml(&doc)).unwrap();
    assert_eq!(again, doc);
    let a = analyze(&again).unwrap();
    assert!(!a.has_errors(), "{:#?}", a.all_issues().collect::<Vec<_>>());
    assert_eq!(a.base_path.as_deref(), Some("/api"));
    let imported = a.endpoints(None).unwrap();
    assert_eq!(imported.len(), 4, "resource operations are skipped");

    // OpenAPI groups operations by path, so compare by name.
    for orig in &endpoints {
        let back = imported.iter().find(|e| e.name == orig.name).unwrap();
        assert_eq!(back.method, orig.method);
        assert_eq!(back.path, orig.path);
        assert_eq!(back.enabled, orig.enabled);
        assert_eq!(back.auth, orig.auth);
        assert_eq!(back.response, orig.response, "{}", orig.name);
        assert_eq!(back.rules, orig.rules);
        assert_eq!(back.request.query, orig.request.query);
    }
    let create = imported.iter().find(|e| e.name == "Create user").unwrap();
    let schema = create.request.body_schema.as_ref().unwrap();
    let v = jsonschema::validator_for(schema).unwrap();
    assert!(v.is_valid(&json!({ "name": "a", "friends": [{ "name": "b" }] })));
    assert!(!v.is_valid(&json!({ "name": "a", "friends": [{}] })));
}

#[test]
fn export_of_empty_project_is_valid() {
    let doc = export(&ApiDefinition {
        title: "Empty",
        description: "Nothing yet",
        version: "0.1.0",
        base_path: None,
        endpoints: &[],
        resources: &[],
    });
    assert_eq!(doc["info"]["description"], "Nothing yet");
    assert!(doc.get("servers").is_none());
    let a = analyze(&doc).unwrap();
    assert!(!a.has_errors());
}

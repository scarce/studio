//! `GET /openapi.json` — the OpenAPI 3.1 description of this surface.
//!
//! Assembled, not hand-maintained: every component schema comes from the
//! published registry (`studio_types::schemas`), i.e. from the same
//! schemars derives that generate `schemas/*.json` — the OpenAPI contract
//! cannot drift from the wire types. OpenAPI 3.1 speaks JSON Schema
//! 2020-12 natively, so the registry schemas embed unchanged except for
//! one mechanical transform: each schema's `$defs` are hoisted into
//! `#/components/schemas` (deduplicated, refs rewritten) so the document
//! is plain-pointer resolvable by tooling that mishandles embedded `$id`.
//!
//! This document is the discovery surface a payment gateway gates against;
//! `tests/openapi_api.rs` holds the drift guards (every documented
//! operation is routed; every advertised endpoint is documented).

use serde_json::{json, Map, Value};

/// Build the document. `public_url` becomes the `servers` entry, so the
/// served description is addressable as deployed (e.g. behind a gateway).
pub fn document(public_url: &str) -> Value {
    json!({
        "openapi": "3.1.0",
        "jsonSchemaDialect": "https://json-schema.org/draft/2020-12/schema",
        "info": {
            "title": "scarce-studio",
            "version": env!("CARGO_PKG_VERSION"),
            "description": "HTTP surface of scarced — RFQ capture, quote issuance, buyer acceptance, and the public project view. Buyer surfaces are free and unauthenticated (ARCHITECTURE.md §2.1); quote issuance is the studio's own door (bearer token). Validation failures return 422 with { errors: [{ field, message }] }. Raw JSON Schemas: GET /api/v1/schemas/{name}.",
            "license": { "name": "Apache-2.0", "identifier": "Apache-2.0" },
        },
        "servers": [{ "url": public_url }],
        // Public by default (buyers never authenticate); the one
        // studio-authenticated operation overrides with studio_bearer.
        "security": [],
        "paths": paths(),
        "components": {
            "schemas": component_schemas(),
            "securitySchemes": {
                "studio_bearer": {
                    "type": "http",
                    "scheme": "bearer",
                    "description": "The studio's own door (SCARCED_STUDIO_TOKEN). Never a buyer surface — buyers never authenticate.",
                },
            },
        },
    })
}

/// Every route the router serves, described. Ordered as in `router()`.
fn paths() -> Value {
    json!({
        "/healthz": {
            "get": {
                "operationId": "healthz",
                "summary": "Liveness + readiness — 200 only when the projection store answers",
                "responses": {
                    "200": json_response("service and store healthy", json!({
                        "type": "object",
                        "required": ["status", "version"],
                        "properties": {
                            "status": { "const": "ok" },
                            "version": { "type": "string" },
                        },
                    })),
                    "503": json_response("projection store unreachable", json!({
                        "type": "object",
                        "properties": { "status": { "const": "degraded" }, "store": { "type": "string" } },
                    })),
                },
            },
        },
        "/openapi.json": {
            "get": {
                "operationId": "openapi",
                "summary": "This document",
                "responses": {
                    "200": json_response("the OpenAPI 3.1 description of this surface", json!({ "type": "object" })),
                },
            },
        },
        "/api/v1": {
            "get": {
                "operationId": "api_index",
                "summary": "Discovery index — endpoints and published schemas, from the base URL alone",
                "responses": {
                    "200": json_response("the index", json!({
                        "type": "object",
                        "required": ["service", "version", "endpoints", "schemas"],
                        "properties": {
                            "service": { "const": "scarce-studio" },
                            "version": { "type": "string" },
                            "endpoints": { "type": "array", "items": { "type": "object" } },
                            "schemas": { "type": "array", "items": { "type": "object" } },
                            "errors": { "type": "string" },
                        },
                    })),
                },
            },
        },
        "/api/v1/schemas/{name}": {
            "get": {
                "operationId": "get_schema",
                "summary": "JSON Schema of a wire type (same values as the checked-in schemas/*.json)",
                "parameters": [path_param("name", "wire name of a published schema (see GET /api/v1)")],
                "responses": {
                    "200": json_response("the JSON Schema", json!({ "type": "object" })),
                    "404": json_response("unknown schema name; body lists the available ones", json!({
                        "type": "object",
                        "required": ["error", "available"],
                        "properties": {
                            "error": { "type": "string" },
                            "available": { "type": "array", "items": { "type": "string" } },
                        },
                    })),
                },
            },
        },
        "/api/v1/rfqs": {
            "post": {
                "operationId": "create_rfq",
                "summary": "Capture a demand record — free, unsigned, frictionless (never tax the order book)",
                "requestBody": {
                    "required": true,
                    "content": { "application/json": { "schema": schema_ref("rfq") } },
                },
                "responses": {
                    "201": ref_response("the captured record, with server-assigned id and created_at", "rfq-record"),
                    "422": ref_response("validation failure", "validation-error"),
                    "500": ref_response("storage failure", "validation-error"),
                },
            },
            "get": {
                "operationId": "list_rfqs",
                "summary": "The order book — captured RFQs, oldest first",
                "parameters": [json!({
                    "name": "since",
                    "in": "query",
                    "required": false,
                    "schema": { "type": "string", "format": "date-time" },
                    "description": "RFC 3339 timestamp; only RFQs captured at or after it are returned",
                })],
                "responses": {
                    "200": json_response("the RFQs", json!({
                        "type": "object",
                        "required": ["rfqs"],
                        "properties": { "rfqs": { "type": "array", "items": schema_ref("rfq-record") } },
                    })),
                    "422": ref_response("malformed since parameter", "validation-error"),
                    "500": ref_response("storage failure", "error"),
                },
            },
        },
        "/api/v1/rfqs/{id}": {
            "get": {
                "operationId": "get_rfq",
                "summary": "Fetch one captured RFQ — free read",
                "parameters": [path_param("id", "RFQ id")],
                "responses": {
                    "200": ref_response("the record", "rfq-record"),
                    "404": ref_response("no such RFQ", "error"),
                    "500": ref_response("storage failure", "error"),
                },
            },
        },
        "/api/v1/rfqs/{id}/quote": {
            "post": {
                "operationId": "create_quote",
                "summary": "Issue the quote for an RFQ — studio-authenticated; fail-closed when no token is configured",
                "security": [{ "studio_bearer": [] }],
                "parameters": [path_param("id", "RFQ id")],
                "requestBody": {
                    "required": true,
                    "content": { "application/json": { "schema": schema_ref("quote") } },
                },
                "responses": {
                    "201": ref_response("the issued quote, with policy_hash and expiry", "quote-record"),
                    "401": ref_response("missing or wrong bearer token", "error"),
                    "404": ref_response("no such RFQ", "error"),
                    "409": ref_response("a quote already exists for this RFQ", "error"),
                    "422": ref_response("validation failure", "validation-error"),
                    "500": ref_response("storage failure", "error"),
                    "503": ref_response("quote issuance disabled (no studio token configured)", "error"),
                },
            },
            "get": {
                "operationId": "get_quote",
                "summary": "The buyer's free read — status fail-closed against expiry (past-expiry reads LAPSED before the sweep stamps it)",
                "parameters": [path_param("id", "RFQ id")],
                "responses": {
                    "200": ref_response("the quote, status as of now", "quote-record"),
                    "404": ref_response("no quote for this RFQ", "error"),
                    "500": ref_response("storage failure", "error"),
                },
            },
        },
        "/api/v1/rfqs/{id}/quote/accept": {
            "post": {
                "operationId": "accept_quote",
                "summary": "Accept a live quote — buyer, free, exactly once; starts the contract",
                "parameters": [path_param("id", "RFQ id")],
                "responses": {
                    "200": ref_response("the accepted quote plus project_url — the buyer's next click", "quote-accepted"),
                    "404": ref_response("no quote exists for this RFQ", "error"),
                    "409": ref_response("already accepted, or lapsed and no longer acceptable", "error"),
                    "500": ref_response("storage failure", "error"),
                },
            },
        },
        "/api/v1/projects/{id}": {
            "get": {
                "operationId": "get_project",
                "summary": "Public project view — deliberately commercial-free; what /project/{id} renders",
                "parameters": [path_param("id", "project id (= RFQ id)")],
                "responses": {
                    "200": ref_response("the public view", "project"),
                    "404": ref_response("no such project", "error"),
                    "500": ref_response("storage failure", "error"),
                },
            },
        },
        "/project/{id}": {
            "get": {
                "operationId": "project_page",
                "summary": "The project page — embedded web shell rendering the public view client-side",
                "parameters": [path_param("id", "project id; existence is the API's answer, unknown ids render the page's own not-found state")],
                "responses": {
                    "200": { "description": "the page shell", "content": { "text/html": {} } },
                },
            },
        },
        "/assets/{file}": {
            "get": {
                "operationId": "asset",
                "summary": "Embedded page assets (css/js/logo), compiled into the binary",
                "parameters": [path_param("file", "asset filename")],
                "responses": {
                    "200": { "description": "the asset", "content": { "*/*": {} } },
                    "404": { "description": "no such asset", "content": { "text/plain": {} } },
                },
            },
        },
    })
}

/// `#/components/schemas`: the published registry, plus the composite
/// response shapes the handlers assemble around it.
fn component_schemas() -> Value {
    let mut components = Map::new();

    // Wrappers first so a registry name could never be silently shadowed —
    // `insert_unique` panics (test-caught, the set is static) on collision.
    insert_unique(
        &mut components,
        "error",
        json!({
            "title": "Error",
            "type": "object",
            "required": ["error"],
            "properties": { "error": { "type": "string" } },
        }),
    );
    insert_unique(
        &mut components,
        "validation-error",
        json!({
            "title": "Validation failure",
            "type": "object",
            "required": ["errors"],
            "properties": {
                "errors": { "type": "array", "items": schema_ref("field-error") },
            },
        }),
    );
    insert_unique(
        &mut components,
        "quote-accepted",
        json!({
            "title": "Accepted quote",
            "allOf": [
                schema_ref("quote-record"),
                {
                    "type": "object",
                    "required": ["project_url"],
                    "properties": {
                        "project_url": {
                            "type": "string",
                            "format": "uri",
                            "description": "The shareable project page — the buyer's next click.",
                        },
                    },
                },
            ],
        }),
    );

    for (name, schema) in studio_types::schemas::all() {
        let hoisted = hoist(schema, &mut components);
        insert_unique(&mut components, name, hoisted);
    }

    Value::Object(components)
}

/// Embed one registry schema: drop the standalone-document keywords
/// (`$schema`, `$id`), hoist its `$defs` into the shared component map, and
/// rewrite internal refs accordingly. Identical defs (the shared types —
/// `Amount`, `GatePolicy`, …) deduplicate; a same-name different-shape def
/// panics (test-caught: the registry is static).
fn hoist(mut schema: Value, components: &mut Map<String, Value>) -> Value {
    let object = schema
        .as_object_mut()
        .expect("registry schema is an object");
    object.remove("$schema");
    object.remove("$id");
    if let Some(Value::Object(defs)) = object.remove("$defs") {
        for (def_name, mut def) in defs {
            rewrite_refs(&mut def);
            match components.get(&def_name) {
                None => {
                    components.insert(def_name, def);
                }
                Some(existing) => assert_eq!(
                    *existing, def,
                    "component {def_name:?} generated with two different shapes"
                ),
            }
        }
    }
    rewrite_refs(&mut schema);
    schema
}

/// `#/$defs/X` → `#/components/schemas/X`, recursively.
fn rewrite_refs(value: &mut Value) {
    match value {
        Value::Object(object) => {
            for (key, entry) in object.iter_mut() {
                if key == "$ref" {
                    if let Some(target) = entry.as_str().and_then(|r| r.strip_prefix("#/$defs/")) {
                        *entry = Value::String(format!("#/components/schemas/{target}"));
                        continue;
                    }
                }
                rewrite_refs(entry);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(rewrite_refs),
        _ => {}
    }
}

fn insert_unique(components: &mut Map<String, Value>, name: &str, schema: Value) {
    let previous = components.insert(name.to_string(), schema);
    assert!(previous.is_none(), "component {name:?} defined twice");
}

fn schema_ref(name: &str) -> Value {
    json!({ "$ref": format!("#/components/schemas/{name}") })
}

fn json_response(description: &str, schema: Value) -> Value {
    json!({
        "description": description,
        "content": { "application/json": { "schema": schema } },
    })
}

fn ref_response(description: &str, component: &str) -> Value {
    json_response(description, schema_ref(component))
}

fn path_param(name: &str, description: &str) -> Value {
    json!({
        "name": name,
        "in": "path",
        "required": true,
        "schema": { "type": "string" },
        "description": description,
    })
}

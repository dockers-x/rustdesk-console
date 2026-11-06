use serde_json::{json, Map, Value};

fn reference(name: &str) -> Value {
    json!({"$ref":format!("#/components/schemas/{name}")})
}
fn object(properties: Value, required: &[&str]) -> Value {
    let mut schema = json!({"type":"object","properties":properties});
    if !required.is_empty() {
        schema["required"] = json!(required);
    }
    schema
}
fn string() -> Value {
    json!({"type":"string"})
}
fn integer() -> Value {
    json!({"type":"integer","format":"int64"})
}

pub fn document() -> Value {
    let mut schemas = Map::new();
    schemas.insert("User".into(), object(json!({"id":integer(),"username":string(),"email":string(),"nickname":string(),"is_admin":{"type":"boolean"},"status":{"type":"integer","enum":[1,2]}}), &["id","username","email","nickname","is_admin","status"]));
    schemas.insert("Device".into(), object(json!({"row_id":integer(),"id":string(),"alias":string(),"hostname":string(),"username":string(),"os":string(),"user_id":integer(),"group_id":integer(),"status":integer(),"last_online_time":integer()}), &["row_id","id","alias","hostname","username","os","user_id","group_id","status","last_online_time"]));
    schemas.insert(
        "DeviceGroup".into(),
        object(
            json!({"id":integer(),"name":string(),"note":string()}),
            &["id", "name", "note"],
        ),
    );
    schemas.insert(
        "Strategy".into(),
        object(json!({"id":integer(),"name":string()}), &["id", "name"]),
    );
    schemas.insert("AddressBook".into(), object(json!({"row_id":integer(),"id":string(),"alias":string(),"hostname":string(),"platform":string(),"note":string()}), &["row_id","id","alias","hostname","platform","note"]));
    schemas.insert("Error".into(), object(json!({"error":object(json!({"code":string(),"message":string()}), &["code","message"])}), &["error"]));
    schemas.insert("DeviceUpdate".into(), object(json!({"alias":{"type":"string","maxLength":255},"user_id":{"type":"integer","minimum":0},"group_id":{"type":"integer","minimum":0}}), &[]));
    schemas.insert("UserCreate".into(), object(json!({"username":{"type":"string","minLength":1,"maxLength":120},"password":{"type":"string","format":"password","writeOnly":true,"minLength":8,"maxLength":72},"email":string(),"nickname":string()}), &["username","password"]));
    schemas.insert(
        "UserUpdate".into(),
        object(
            json!({"email":string(),"nickname":string(),"status":{"type":"integer","enum":[1,2]}}),
            &[],
        ),
    );
    schemas.insert(
        "DeviceGroupCreate".into(),
        object(
            json!({"name":{"type":"string","minLength":1,"maxLength":120},"note":string()}),
            &["name"],
        ),
    );
    schemas.insert("AddressBookCreate".into(), object(json!({"id":{"type":"string","minLength":1,"maxLength":255},"alias":string(),"hostname":string(),"platform":string(),"note":string()}), &["id"]));
    for name in [
        "DeviceUpdate",
        "UserCreate",
        "UserUpdate",
        "DeviceGroupCreate",
        "AddressBookCreate",
    ] {
        schemas[name]["additionalProperties"] = json!(false);
    }
    let mut paths = Map::<String, Value>::new();
    for (path, method, operation, scope, model, list, input, admin, description) in [
        ("/me", "get", "getMe", "profile:read", "User", false, "", false, "Read the API key owner's account."),
        ("/devices", "get", "listDevices", "devices:read", "Device", true, "", false, "Administrators see all devices. Other accounts see only devices assigned to their own account. Optional id is the RustDesk connection ID."),
        ("/devices/{id}", "get", "getDevice", "devices:read", "Device", false, "", false, "Read a Console device row ID. Non-administrators cannot read another account's device."),
        ("/devices/{id}", "patch", "updateDevice", "devices:write", "Device", false, "DeviceUpdate", true, "Update alias or assign an existing user/device group. Zero unassigns user or group."),
        ("/devices/{id}", "delete", "deleteDevice", "devices:write", "", false, "", true, "Delete a device using existing Console token-cleanup behavior."),
        ("/users", "get", "listUsers", "users:read", "User", true, "", true, "List users without passwords, TOTP secrets or login tokens."),
        ("/users/{id}", "get", "getUser", "users:read", "User", false, "", true, "Read a user by numeric Console row ID."),
        ("/users", "post", "createUser", "users:write", "User", false, "UserCreate", true, "Create an enabled, non-administrator account. Password is 8 to 72 UTF-8 bytes; role-escalation fields are rejected."),
        ("/users/{id}", "patch", "updateUser", "users:write", "User", false, "UserUpdate", true, "Update email, nickname or enabled status. Last-administrator protection is preserved."),
        ("/users/{id}", "delete", "deleteUser", "users:write", "", false, "", true, "Delete a user using existing Console cleanup behavior. The API key owner cannot delete itself."),
        ("/device-groups", "get", "listDeviceGroups", "device-groups:read", "DeviceGroup", true, "", true, "List device groups."),
        ("/device-groups", "post", "createDeviceGroup", "device-groups:write", "DeviceGroup", false, "DeviceGroupCreate", true, "Create a device group with optional note."),
        ("/strategies", "get", "listStrategies", "strategies:read", "Strategy", true, "", true, "Discover strategy IDs and names. Strategy editing is not part of this API version."),
        ("/address-books", "get", "listAddressBooks", "address-books:read", "AddressBook", true, "", false, "List only the key owner's address-book entries, including for administrators. Passwords and stored hashes are omitted."),
        ("/address-books", "post", "createAddressBook", "address-books:write", "AddressBook", false, "AddressBookCreate", false, "Create an entry for the key owner in legacy collection 0. No cross-account assignment."),
        ("/address-books/{id}", "delete", "deleteAddressBook", "address-books:write", "", false, "", false, "Delete only the key owner's entry by numeric Console row ID."),
    ] {
        let response_schema = if list { object(json!({"data":{"type":"array","items":reference(model)},"total":integer(),"page":integer(),"page_size":integer()}), &["data","total","page","page_size"]) } else { object(json!({"data":if model.is_empty() { json!({"type":"object","nullable":true,"enum":[null]}) } else { reference(model) }}), &["data"]) };
        let status = if method == "post" { "201" } else { "200" };
        let mut responses = Map::new();
        responses.insert(status.into(), json!({"description":"Success","content":{"application/json":{"schema":response_schema}}}));
        for (status, description) in [("400","Invalid request or operation refused"),("401","Missing, invalid, expired or revoked key; owner disabled, deleted or required to change password"),("403","Insufficient scope or owner no longer an administrator"),("404","Resource not found or not visible to the key owner"),("409","Resource conflict"),("500","Database or internal failure")] { responses.insert(status.into(), json!({"description":description,"content":{"application/json":{"schema":reference("Error")}}})); }
        let mut parameters = Vec::new();
        if path.contains("{id}") { parameters.push(json!({"name":"id","in":"path","required":true,"description":"Positive numeric Console row ID, not the RustDesk connection ID","schema":{"type":"integer","minimum":1}})); }
        if list {
            parameters.push(json!({"name":"page","in":"query","schema":{"type":"integer","minimum":1,"default":1}}));
            parameters.push(json!({"name":"page_size","in":"query","schema":{"type":"integer","minimum":1,"maximum":100,"default":30}}));
            if path == "/devices" { parameters.push(json!({"name":"id","in":"query","description":"Exact RustDesk connection ID","schema":{"type":"string"}})); }
        }
        let mut entry = json!({"operationId":operation,"summary":operation,"description":description,"security":[{"ApiKey":[]}],"x-required-scope":scope,"x-admin-only":admin,"parameters":parameters,"responses":responses});
        if !input.is_empty() { entry["requestBody"] = json!({"required":true,"content":{"application/json":{"schema":reference(input)}}}); }
        paths.entry(path.to_string()).or_insert_with(|| json!({}))[method] = entry;
    }
    json!({"openapi":"3.0.3","info":{"title":"RustDesk Console Developer API","version":"1.0.0","description":"Scoped API keys for programmatic Console management. API keys are independent of login/deployment tokens and are accepted only on this versioned API. Account state and administrative role are rechecked on each request. Keys cannot manage other API keys. Configure your own Console base URL; use TLS in production."},"servers":[{"url":"/api/v1"}],"paths":paths,"components":{"securitySchemes":{"ApiKey":{"type":"http","scheme":"bearer","bearerFormat":"API key","description":"An rdc_ API key created in Console Developer access. Keep it secret; never put it in a URL."}},"schemas":schemas}})
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specification_covers_every_management_operation_and_resolves_schema_refs() {
        let doc = document();
        assert_eq!(doc["openapi"], "3.0.3");
        assert_eq!(
            doc["paths"]
                .as_object()
                .unwrap()
                .values()
                .map(|path| path.as_object().unwrap().len())
                .sum::<usize>(),
            16
        );
        fn check(value: &Value, schemas: &Value) {
            match value {
                Value::Object(fields) => {
                    if let Some(required) = fields.get("required").and_then(Value::as_array) {
                        assert!(
                            !required.is_empty(),
                            "OpenAPI 3.0 forbids an empty required array"
                        );
                    }
                    if let Some(reference) = fields.get("$ref") {
                        let name = reference
                            .as_str()
                            .unwrap()
                            .strip_prefix("#/components/schemas/")
                            .unwrap();
                        assert!(schemas.get(name).is_some(), "Missing schema {name}");
                    }
                    for child in fields.values() {
                        check(child, schemas);
                    }
                }
                Value::Array(items) => {
                    for child in items {
                        check(child, schemas);
                    }
                }
                _ => {}
            }
        }
        check(&doc, &doc["components"]["schemas"]);
    }
}

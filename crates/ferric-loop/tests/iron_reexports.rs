//! T-12601 (INT-0011 AC-2): the constrained protocol moved to `ferric-iron`,
//! and every old public path still resolves — to the *same* item, not a copy.
//! Each binding below is typed with the `ferric_iron` item and initialized
//! through the old path, so a divergent duplicate would fail to compile.

use serde_json::json;

#[test]
fn reexport_paths_resolve() {
    let descriptor: ferric_iron::ToolDescriptor = ferric_provider::ToolDescriptor {
        name: "read_file".to_string(),
        description: "Read a file.".to_string(),
        input_schema: json!({"type": "object"}),
    };
    let _: ferric_iron::Constraint = ferric_provider::Constraint::JsonSchema(json!({}));
    let _: ferric_iron::StreamDelta = ferric_provider::StreamDelta::ToolNamed("x".to_string());
    let caps: ferric_iron::Capabilities = ferric_provider::Capabilities {
        supports_native_tool_calls: false,
        supports_constraint: true,
        exposes_logits: false,
        supports_media: false,
    };
    let _: ferric_iron::ConstrainedJsonScanner = ferric_provider::ConstrainedJsonScanner::new();

    let schema = ferric_loop::action_schema(std::slice::from_ref(&descriptor));
    assert_eq!(schema, ferric_iron::action_schema(&[descriptor]));
    let parsed: Result<_, ferric_iron::ActionParseError> =
        ferric_loop::parse_json_action(0, r#"{"thought":"t","tool":"read_file","args":{}}"#);
    assert_eq!(parsed.unwrap().name, "read_file");

    let policy = ferric_core::policy_for(&ferric_core::ModelProfile {
        params_b: 1.0,
        quant: "Q4_K_M".to_string(),
        ctx: 4096,
        family: "t".to_string(),
        measured_level: None,
    });
    assert_eq!(
        ferric_loop::select_protocol(&policy, &caps, None),
        ferric_iron::select_protocol(&policy, &caps, None)
    );

    let controls = ferric_loop::control_descriptors(ferric_core::ActionProtocol::ConstrainedJson);
    assert_eq!(
        controls,
        ferric_iron::terminator::control_descriptors(ferric_core::ActionProtocol::ConstrainedJson)
    );
    assert_eq!(
        ferric_loop::TASK_COMPLETE,
        ferric_iron::terminator::TASK_COMPLETE
    );
    assert_eq!(
        ferric_loop::SUBMIT_PLAN,
        ferric_iron::terminator::SUBMIT_PLAN
    );
    assert_eq!(
        ferric_loop::REQUEST_USER_INPUT,
        ferric_iron::terminator::REQUEST_USER_INPUT
    );
    assert!(ferric_loop::is_request_user_input(
        ferric_loop::request_user_input_descriptor().name.as_str()
    ));
    let rejected: Result<_, ferric_iron::terminator::UserInputRequestError> =
        ferric_loop::request_of(&json!("not an object"));
    assert!(rejected.is_err());
}

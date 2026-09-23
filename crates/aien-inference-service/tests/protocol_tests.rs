use aien_inference_service::{InferenceEvent, InferenceRequest};

#[test]
fn test_inference_request_builder_and_defaults() {
    let req = InferenceRequest::new("Hello from Sovereign Core")
        .with_system_prompt("You are Atlas")
        .with_max_tokens(512)
        .with_temperature(0.2);

    assert_eq!(req.prompt, "Hello from Sovereign Core");
    assert_eq!(req.system_prompt.as_deref(), Some("You are Atlas"));
    assert_eq!(req.limits.max_tokens, 512);
    assert_eq!(req.limits.temperature, 0.2);

    let json = serde_json::to_string(&req).expect("serialize request");
    let deserialized: InferenceRequest = serde_json::from_str(&json).expect("deserialize request");
    assert_eq!(deserialized.prompt, req.prompt);
}

#[test]
fn test_inference_event_variants() {
    let tok_evt = InferenceEvent::Token("fragment".to_string());
    let json = serde_json::to_string(&tok_evt).expect("serialize token event");
    assert!(json.contains("\"type\":\"Token\""));

    let comp_evt = InferenceEvent::Completed {
        total_tokens: 10,
        prompt_tokens: 4,
        completion_tokens: 6,
    };
    let json = serde_json::to_string(&comp_evt).expect("serialize completed event");
    assert!(json.contains("\"type\":\"Completed\""));
}

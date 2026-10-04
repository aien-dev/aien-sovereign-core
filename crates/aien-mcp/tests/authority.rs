//! Negative controls for the host-owned authority seam. This file is outside the crate, so it
//! can touch only the public API: no `AuthorizedEffect` can be built here except through
//! `EffectLane::authorize`.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use aien_capability::{
    Digest32, EffectId, JNodeId, ProviderId, ToolDescriptor, ToolEffects, WorldId,
};
use aien_mcp::memory::MemoryWire;
use aien_mcp::{
    AuthorityContext, AuthorityDecision, AuthorityOutcome, CallOutcome, EffectAuthority,
    EffectClassAuthority, EffectIntent, EffectLane, EffectScope, McpBroker,
};
use serde_json::json;

struct Fixed(AuthorityDecision);
impl EffectAuthority for Fixed {
    fn authorize(&self, _: &EffectIntent, _: &AuthorityContext) -> AuthorityDecision {
        self.0.clone()
    }
}

struct H {
    lane: EffectLane,
    calls: Arc<AtomicUsize>,
    intent: EffectIntent,
}

fn scope(label: &str) -> EffectScope {
    EffectScope {
        world_id: WorldId(1),
        winning_jnode: JNodeId(1),
        idempotency_key: EffectId::from_label(label),
    }
}

async fn harness(effects: ToolEffects) -> H {
    let calls = Arc::new(AtomicUsize::new(0));
    let c = calls.clone();
    let tool = ToolDescriptor::new("t", effects, Digest32::of(b"t"));
    let wire = MemoryWire::new(vec![tool], move |_, _| {
        c.fetch_add(1, Ordering::SeqCst);
        CallOutcome::Finished(json!({"done": true}))
    });
    let broker = McpBroker::new();
    let provider = ProviderId::new("p");
    broker
        .admit(provider.clone(), Arc::new(wire))
        .await
        .unwrap();
    // An intent is plain data (public fields). Possessing one grants nothing.
    let live = aien_mcp::SpeculativeLane::new(broker.clone())
        .discovery_snapshot(&provider)
        .await
        .unwrap()
        .catalog_digest;
    let intent = EffectIntent {
        provider,
        tool_name: "t".into(),
        arguments: json!({"a": 1}),
        capability_digest: live,
    };
    H {
        lane: EffectLane::new(broker),
        calls,
        intent,
    }
}

#[tokio::test]
async fn deny_mints_nothing_and_never_executes() {
    let h = harness(ToolEffects::PURE).await;
    let a = Fixed(AuthorityDecision::Deny("no".into()));
    assert!(h.lane.authorize(h.intent.clone(), scope("d"), &a).is_err());
    let r = h.lane.authorize_and_execute(h.intent, scope("d"), &a).await;
    assert_eq!(r.unwrap_err(), AuthorityOutcome::Denied("no".into()));
    assert_eq!(h.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn contain_mints_nothing_and_never_executes() {
    let h = harness(ToolEffects::PURE).await;
    let a = Fixed(AuthorityDecision::Contain("box".into()));
    let r = h.lane.authorize_and_execute(h.intent, scope("c"), &a).await;
    assert_eq!(r.unwrap_err(), AuthorityOutcome::Contained("box".into()));
    assert_eq!(h.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn require_approval_is_pending_and_never_executes() {
    let h = harness(ToolEffects::WORLD_MUTATION).await;
    let want = aien_mcp::intent_digest(&h.intent);
    let r = h
        .lane
        .authorize_and_execute(h.intent, scope("p"), &EffectClassAuthority)
        .await;
    match r.unwrap_err() {
        AuthorityOutcome::Pending { intent_digest, .. } => assert_eq!(intent_digest, want),
        other => panic!("expected Pending, got {other:?}"),
    }
    assert_eq!(h.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn allow_executes_and_returns_receipt() {
    let h = harness(ToolEffects::LOCAL_EPHEMERAL).await;
    let r = h
        .lane
        .authorize_and_execute(h.intent, scope("a"), &EffectClassAuthority)
        .await
        .unwrap();
    assert_eq!(r.output, json!({"done": true}));
    assert_eq!(r.tool_name, "t");
    assert_eq!(h.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn allow_restricted_mints() {
    let h = harness(ToolEffects::PURE).await;
    let a = Fixed(AuthorityDecision::AllowRestricted(vec!["x".into()]));
    assert!(h
        .lane
        .authorize_and_execute(h.intent, scope("r"), &a)
        .await
        .is_ok());
}

#[tokio::test]
async fn unknown_descriptor_is_denied() {
    let h = harness(ToolEffects::PURE).await;
    let mut intent = h.intent.clone();
    intent.tool_name = "ghost".into();
    let r = h
        .lane
        .authorize_and_execute(intent, scope("u"), &EffectClassAuthority)
        .await;
    assert!(matches!(r.unwrap_err(), AuthorityOutcome::Denied(_)));
    assert_eq!(h.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn stale_intent_is_denied_before_the_authority_is_asked() {
    let h = harness(ToolEffects::PURE).await;
    let mut intent = h.intent.clone();
    intent.capability_digest = Digest32::of(b"old");
    let a = Fixed(AuthorityDecision::Allow);
    let r = h.lane.authorize_and_execute(intent, scope("s"), &a).await;
    assert!(matches!(r.unwrap_err(), AuthorityOutcome::Denied(_)));
    assert_eq!(h.calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn effect_class_table() {
    use AuthorityDecision as D;
    let cases = [
        (ToolEffects::PURE, "allow"),
        (ToolEffects::READ_FILESYSTEM, "allow"),
        (ToolEffects::READ_NETWORK, "allow"),
        (ToolEffects::LOCAL_EPHEMERAL, "allow"),
        (ToolEffects::EXTERNAL_WRITE, "approval"),
        (ToolEffects::WORLD_MUTATION, "approval"),
        (ToolEffects::SECRET_BEARING, "approval"),
        (ToolEffects::SPAWN_PROCESS, "approval"),
        (ToolEffects::EXTERNAL_IRREVERSIBLE, "deny"),
        (
            ToolEffects::EXTERNAL_IRREVERSIBLE | ToolEffects::READ_FILESYSTEM,
            "deny",
        ),
    ];
    for (effects, want) in cases {
        let h = harness(effects).await;
        let r = h
            .lane
            .authorize(h.intent.clone(), scope("t"), &EffectClassAuthority);
        let got = match &r {
            Ok(_) => "allow",
            Err(AuthorityOutcome::Pending { .. }) => "approval",
            Err(AuthorityOutcome::Denied(_)) => "deny",
            Err(_) => "other",
        };
        assert_eq!(got, want, "{effects:?}");
    }
    let _ = D::Allow;
}

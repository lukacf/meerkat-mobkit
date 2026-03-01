use std::sync::Arc;
use std::time::Duration;

use futures::StreamExt;
use meerkat::{build_ephemeral_service, AgentEvent, AgentFactory, Config};
use meerkat_mob::{MeerkatId, MobBuilder, MobStorage, Prefab, ProfileName};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let _ =
        std::env::var("OPENAI_API_KEY").map_err(|_| "Set OPENAI_API_KEY to run streaming_smoke")?;
    let model = std::env::var("RKAT_SMOKE_MODEL").unwrap_or_else(|_| "gpt-5.2".to_string());

    println!("STREAM_SMOKE: model={model}");

    let temp_dir = tempfile::tempdir()?;
    let store_path = temp_dir.path().join("sessions");
    std::fs::create_dir_all(&store_path)?;

    let factory = AgentFactory::new(&store_path).comms(true);
    let config = Config::default();
    let session_service = Arc::new(build_ephemeral_service(factory, config, 16));

    let mut definition = Prefab::CodingSwarm.definition();
    for profile in definition.profiles.values_mut() {
        profile.model = model.clone();
    }

    let handle = MobBuilder::new(definition, MobStorage::in_memory())
        .with_session_service(session_service)
        .allow_ephemeral_sessions(true)
        .create()
        .await?;

    handle
        .spawn(
            ProfileName::from("lead"),
            MeerkatId::from("lead-1"),
            Some("You are concise. 1 short paragraph max.".to_string()),
        )
        .await?;
    handle
        .spawn(
            ProfileName::from("worker"),
            MeerkatId::from("worker-1"),
            Some("You are a helper worker.".to_string()),
        )
        .await?;
    handle
        .wire(MeerkatId::from("lead-1"), MeerkatId::from("worker-1"))
        .await?;
    println!("STREAM_SMOKE: spawned + wired lead-1 <-> worker-1");

    // 1) Interaction-scoped stream (inject_and_subscribe)
    let mut interaction = handle
        .inject_and_subscribe(
            MeerkatId::from("lead-1"),
            "Give a short plan for smoke-testing a Rust CLI and delegate one concrete task to worker-1."
                .to_string(),
        )
        .await?;
    println!("STREAM_SMOKE: interaction_id={}", interaction.id);

    // 2) Per-agent session event stream
    let mut agent_stream = handle
        .subscribe_agent_events(&MeerkatId::from("lead-1"))
        .await?;
    println!("STREAM_SMOKE: subscribed to lead-1 agent events");

    // 3) Mob-wide merged attributed event stream
    let mut mob_router = handle.subscribe_mob_events();
    println!("STREAM_SMOKE: subscribed to mob event router");

    let deadline = tokio::time::Instant::now() + Duration::from_secs(35);
    let mut seen_terminal = false;
    let mut interaction_count = 0_u32;
    let mut agent_count = 0_u32;
    let mut mob_count = 0_u32;

    while tokio::time::Instant::now() < deadline && !seen_terminal {
        tokio::select! {
            maybe_evt = interaction.events.recv() => {
                if let Some(evt) = maybe_evt {
                    interaction_count += 1;
                    match evt {
                        AgentEvent::TextDelta { delta } => {
                            println!("INTERACTION text_delta: {}", delta.replace('\n', " "));
                        }
                        AgentEvent::TextComplete { content } => {
                            println!("INTERACTION text_complete: {}", content.replace('\n', " "));
                        }
                        AgentEvent::ToolCallRequested { name, .. } => {
                            println!("INTERACTION tool_call_requested: {name}");
                        }
                        AgentEvent::ToolResultReceived { name, is_error, .. } => {
                            println!("INTERACTION tool_result: name={name} is_error={is_error}");
                        }
                        AgentEvent::InteractionComplete { result, .. } => {
                            println!("INTERACTION complete: {}", result.replace('\n', " "));
                            seen_terminal = true;
                        }
                        AgentEvent::InteractionFailed { error, .. } => {
                            println!("INTERACTION failed: {error}");
                            seen_terminal = true;
                        }
                        other => {
                            println!("INTERACTION other: {:?}", other);
                        }
                    }
                }
            }
            maybe_agent_evt = agent_stream.next() => {
                if let Some(envelope) = maybe_agent_evt {
                    agent_count += 1;
                    println!(
                        "AGENT_STREAM {} seq={} source={} payload={:?}",
                        envelope.event_id,
                        envelope.seq,
                        envelope.source_id,
                        envelope.payload,
                    );
                }
            }
            maybe_mob_evt = mob_router.event_rx.recv() => {
                if let Some(attr) = maybe_mob_evt {
                    mob_count += 1;
                    println!(
                        "MOB_ROUTER source={} profile={} payload={:?} seq={}",
                        attr.source,
                        attr.profile,
                        attr.envelope.payload,
                        attr.envelope.seq,
                    );
                }
            }
            _ = tokio::time::sleep(Duration::from_millis(200)) => {}
        }
    }

    println!(
        "STREAM_SMOKE: counts interaction={} agent_stream={} mob_router={}",
        interaction_count, agent_count, mob_count
    );

    mob_router.cancel();
    handle.retire_all().await?;
    Ok(())
}

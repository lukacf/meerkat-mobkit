use std::thread;

use meerkat_mobkit_core::{SharedSseRingBuffer, SseRingBuffer, SseStoredEvent};

fn stored(id: &str, event_type: &str, data: &str) -> SseStoredEvent {
    SseStoredEvent {
        id: id.to_string(),
        event_type: event_type.to_string(),
        data: data.to_string(),
    }
}

// ---------------------------------------------------------------------------
// SseRingBuffer basics
// ---------------------------------------------------------------------------

#[test]
fn ring_buffer_push_and_len() {
    let mut buf = SseRingBuffer::new(8);
    assert!(buf.is_empty());
    assert_eq!(buf.len(), 0);

    buf.push(stored("a:0", "start", "{}"));
    assert_eq!(buf.len(), 1);

    buf.push(stored("a:1", "delta", "hi"));
    assert_eq!(buf.len(), 2);
}

#[test]
fn ring_buffer_evicts_oldest_at_capacity() {
    let mut buf = SseRingBuffer::new(3);
    buf.push(stored("a:0", "start", "{}"));
    buf.push(stored("a:1", "delta", "one"));
    buf.push(stored("a:2", "delta", "two"));
    assert_eq!(buf.len(), 3);

    // Pushing a fourth should evict a:0.
    buf.push(stored("a:3", "done", "three"));
    assert_eq!(buf.len(), 3);
    assert!(!buf.contains("a:0"));
    assert!(buf.contains("a:1"));
    assert!(buf.contains("a:2"));
    assert!(buf.contains("a:3"));
}

#[test]
fn ring_buffer_zero_capacity_never_stores() {
    let mut buf = SseRingBuffer::new(0);
    buf.push(stored("a:0", "start", "{}"));
    assert!(buf.is_empty());
    assert_eq!(buf.len(), 0);
}

// ---------------------------------------------------------------------------
// replay_after
// ---------------------------------------------------------------------------

#[test]
fn replay_after_returns_subsequent_events() {
    let mut buf = SseRingBuffer::new(10);
    buf.push(stored("x:0", "start", "s"));
    buf.push(stored("x:1", "delta", "d1"));
    buf.push(stored("x:2", "delta", "d2"));
    buf.push(stored("x:3", "done", "fin"));

    let replayed = buf.replay_after("x:1").expect("ID should be found");
    let ids: Vec<&str> = replayed.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, vec!["x:2", "x:3"]);
}

#[test]
fn replay_after_last_event_returns_empty() {
    let mut buf = SseRingBuffer::new(10);
    buf.push(stored("x:0", "start", "s"));
    buf.push(stored("x:1", "done", "d"));

    let replayed = buf.replay_after("x:1").expect("ID should be found");
    assert!(replayed.is_empty());
}

#[test]
fn replay_after_unknown_id_returns_none() {
    let mut buf = SseRingBuffer::new(10);
    buf.push(stored("x:0", "start", "s"));
    buf.push(stored("x:1", "done", "d"));

    assert!(buf.replay_after("unknown:99").is_none());
}

#[test]
fn replay_after_first_event_returns_all_remaining() {
    let mut buf = SseRingBuffer::new(10);
    buf.push(stored("x:0", "start", "s"));
    buf.push(stored("x:1", "delta", "d1"));
    buf.push(stored("x:2", "done", "d2"));

    let replayed = buf.replay_after("x:0").expect("ID should be found");
    let ids: Vec<&str> = replayed.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, vec!["x:1", "x:2"]);
}

// ---------------------------------------------------------------------------
// contains
// ---------------------------------------------------------------------------

#[test]
fn contains_returns_true_for_present_ids() {
    let mut buf = SseRingBuffer::new(10);
    buf.push(stored("a:0", "start", "{}"));
    assert!(buf.contains("a:0"));
    assert!(!buf.contains("a:1"));
}

// ---------------------------------------------------------------------------
// SharedSseRingBuffer
// ---------------------------------------------------------------------------

#[test]
fn shared_buffer_push_and_len() {
    let shared = SharedSseRingBuffer::with_capacity(10);
    assert_eq!(shared.len(), 0);
    assert!(shared.is_empty());

    shared.push(stored("a:0", "start", "{}"));
    assert_eq!(shared.len(), 1);
}

#[test]
fn shared_buffer_default_capacity() {
    let shared = SharedSseRingBuffer::new();
    for i in 0..2001 {
        shared.push(stored(&format!("a:{i}"), "delta", "x"));
    }
    // Default capacity is 2000, so the oldest should have been evicted.
    assert_eq!(shared.len(), 2000);
}

#[test]
fn shared_buffer_replay_after_found() {
    let shared = SharedSseRingBuffer::with_capacity(10);
    shared.push(stored("x:0", "start", "s"));
    shared.push(stored("x:1", "delta", "d1"));
    shared.push(stored("x:2", "done", "d2"));

    let result = shared.replay_after("x:0");
    assert!(result.is_ok());
    let events = result.unwrap();
    let ids: Vec<&str> = events.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids, vec!["x:1", "x:2"]);
}

#[test]
fn shared_buffer_replay_after_not_found_is_gap() {
    let shared = SharedSseRingBuffer::with_capacity(3);
    shared.push(stored("x:0", "start", "s"));
    shared.push(stored("x:1", "delta", "d1"));
    shared.push(stored("x:2", "delta", "d2"));
    // Push one more to evict x:0
    shared.push(stored("x:3", "done", "fin"));

    let result = shared.replay_after("x:0");
    assert!(result.is_err(), "evicted ID should produce a replay gap");
}

#[test]
fn shared_buffer_replay_after_last_returns_empty_ok() {
    let shared = SharedSseRingBuffer::with_capacity(10);
    shared.push(stored("x:0", "start", "s"));
    shared.push(stored("x:1", "done", "d"));

    let result = shared.replay_after("x:1");
    assert!(result.is_ok());
    assert!(result.unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Thread safety
// ---------------------------------------------------------------------------

#[test]
fn shared_buffer_concurrent_access() {
    let shared = SharedSseRingBuffer::with_capacity(1000);
    let num_threads = 8;
    let events_per_thread = 200;

    let handles: Vec<_> = (0..num_threads)
        .map(|t| {
            let buf = shared.clone();
            thread::spawn(move || {
                for i in 0..events_per_thread {
                    buf.push(stored(
                        &format!("t{t}:{i}"),
                        "delta",
                        &format!("thread {t} event {i}"),
                    ));
                }
            })
        })
        .collect();

    for h in handles {
        h.join().expect("thread panicked");
    }

    // Total pushed = 8 * 200 = 1600, capacity is 1000
    assert_eq!(shared.len(), 1000);
}

#[test]
fn shared_buffer_concurrent_read_write() {
    let shared = SharedSseRingBuffer::with_capacity(500);

    // Pre-populate
    for i in 0..100 {
        shared.push(stored(&format!("pre:{i}"), "delta", "x"));
    }

    let writer = {
        let buf = shared.clone();
        thread::spawn(move || {
            for i in 100..400 {
                buf.push(stored(&format!("w:{i}"), "delta", "x"));
            }
        })
    };

    let reader = {
        let buf = shared.clone();
        thread::spawn(move || {
            // Repeatedly read -- should never panic.
            for _ in 0..200 {
                let _ = buf.replay_after("pre:50");
                let _ = buf.len();
            }
        })
    };

    writer.join().expect("writer panicked");
    reader.join().expect("reader panicked");
}

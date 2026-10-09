use chrono::{NaiveDate, NaiveDateTime};
use diesel_migrations::MigrationHarness;

use super::*;

/// Builds an in-memory SQLite database with all migrations applied.
fn test_connection() -> SqliteConnection {
    let mut conn =
        SqliteConnection::establish(":memory:").expect("in-memory sqlite connection should open");
    conn.run_pending_migrations(::persistence::MIGRATIONS)
        .expect("migrations should run");
    conn
}

fn task_with_user_query(task_id: &str, query: &str, description: &str) -> api::Task {
    api::Task {
        id: task_id.to_string(),
        description: description.to_string(),
        dependencies: None,
        messages: vec![api::Message {
            id: format!("{task_id}-user-query"),
            task_id: task_id.to_string(),
            message: Some(api::message::Message::UserQuery(api::message::UserQuery {
                query: query.to_string(),
                ..Default::default()
            })),
            ..Default::default()
        }],
        summary: String::new(),
        server_data: String::new(),
    }
}

fn empty_conversation_data() -> AgentConversationData {
    serde_json::from_str(r#"{"server_conversation_token":null}"#)
        .expect("minimal conversation data should deserialize")
}

fn summary_column(conn: &mut SqliteConnection, conversation: &str) -> Option<String> {
    use schema::agent_conversations::dsl::*;
    agent_conversations
        .filter(conversation_id.eq(conversation))
        .select(summary)
        .first::<Option<String>>(conn)
        .expect("conversation row should exist")
}

fn last_modified_column(conn: &mut SqliteConnection, conversation: &str) -> NaiveDateTime {
    use schema::agent_conversations::dsl::*;
    agent_conversations
        .filter(conversation_id.eq(conversation))
        .select(last_modified_at)
        .first::<NaiveDateTime>(conn)
        .expect("conversation row should exist")
}

#[test]
fn upsert_writes_summary_and_metadata_read_skips_tasks() {
    let mut conn = test_connection();
    let task = task_with_user_query("task-1", "Initial query", "Root title");
    upsert_agent_conversation(&mut conn, "conv-1", [&task], empty_conversation_data())
        .expect("upsert should succeed");

    let (conversations, backfills) =
        read_agent_conversation_metadata(&mut conn).expect("metadata read should succeed");

    assert!(
        backfills.is_empty(),
        "rows written with a summary must not be backfilled"
    );
    assert_eq!(conversations.len(), 1);
    assert!(
        conversations[0].tasks.is_empty(),
        "metadata read must not hydrate task payloads"
    );
    let summary: AgentConversationSummary = serde_json::from_str(
        conversations[0]
            .conversation
            .summary
            .as_deref()
            .expect("summary column should be written at upsert time"),
    )
    .expect("summary column should hold valid summary JSON");
    assert_eq!(summary.initial_query, "Initial query");
    assert_eq!(summary.title, "Root title");
    assert!(summary.is_restorable);
}

#[test]
fn metadata_read_derives_and_backfill_persists_summary_for_legacy_rows() {
    use schema::agent_conversations::dsl::*;

    let mut conn = test_connection();
    let task = task_with_user_query("task-1", "Initial query", "Root title");
    upsert_agent_conversation(&mut conn, "conv-1", [&task], empty_conversation_data())
        .expect("upsert should succeed");

    // Simulate a row written before the summary column existed. Setting
    // `last_modified_at` explicitly keeps the update trigger from bumping it.
    let legacy_ts = ts(1_000);
    diesel::update(agent_conversations.filter(conversation_id.eq("conv-1")))
        .set((summary.eq(None::<String>), last_modified_at.eq(legacy_ts)))
        .execute(&mut conn)
        .expect("legacy row setup should succeed");

    let (conversations, backfills) =
        read_agent_conversation_metadata(&mut conn).expect("metadata read should succeed");

    // The read derives the summary from the row's own task snapshot.
    assert_eq!(conversations.len(), 1);
    let derived: AgentConversationSummary = serde_json::from_str(
        conversations[0]
            .conversation
            .summary
            .as_deref()
            .expect("legacy rows should get a read-time-derived summary"),
    )
    .expect("derived summary should be valid JSON");
    assert_eq!(derived.initial_query, "Initial query");

    // ... and queues a backfill preserving the original timestamp.
    assert_eq!(backfills.len(), 1);
    assert_eq!(backfills[0].conversation_id, "conv-1");
    assert_eq!(backfills[0].last_modified_at, legacy_ts);

    backfill_conversation_summaries(&mut conn, backfills).expect("backfill should succeed");

    assert!(
        summary_column(&mut conn, "conv-1").is_some(),
        "backfill must persist the derived summary"
    );
    assert_eq!(
        last_modified_column(&mut conn, "conv-1"),
        legacy_ts,
        "backfill must not reorder history by bumping last_modified_at"
    );

    // Subsequent startups stay metadata-only.
    let (_, backfills) =
        read_agent_conversation_metadata(&mut conn).expect("metadata read should succeed");
    assert!(backfills.is_empty());
}

#[test]
fn backfill_never_overwrites_a_newer_summary() {
    let mut conn = test_connection();
    let task = task_with_user_query("task-1", "Initial query", "Root title");
    upsert_agent_conversation(&mut conn, "conv-1", [&task], empty_conversation_data())
        .expect("upsert should succeed");
    let written_summary = summary_column(&mut conn, "conv-1");
    let written_ts = last_modified_column(&mut conn, "conv-1");

    // Stale backfills (computed before a newer write landed) must not
    // clobber the row's summary or timestamp, regardless of whether the
    // reader observed a NULL or a since-replaced invalid value.
    let stale_from_null = ConversationSummaryBackfill {
        conversation_id: "conv-1".to_string(),
        summary_json: "{\"stale\":true}".to_string(),
        previous_summary: None,
        last_modified_at: ts(1),
    };
    let stale_from_invalid = ConversationSummaryBackfill {
        conversation_id: "conv-1".to_string(),
        summary_json: "{\"stale\":true}".to_string(),
        previous_summary: Some("{not valid json".to_string()),
        last_modified_at: ts(1),
    };
    backfill_conversation_summaries(&mut conn, vec![stale_from_null, stale_from_invalid])
        .expect("backfill should succeed");

    assert_eq!(summary_column(&mut conn, "conv-1"), written_summary);
    assert_eq!(last_modified_column(&mut conn, "conv-1"), written_ts);
}

#[test]
fn metadata_read_heals_invalid_non_null_summaries() {
    use schema::agent_conversations::dsl::*;

    let mut conn = test_connection();
    let task = task_with_user_query("task-1", "Initial query", "Root title");
    upsert_agent_conversation(&mut conn, "conv-1", [&task], empty_conversation_data())
        .expect("upsert should succeed");

    // Corrupt the summary with unparseable JSON.
    let legacy_ts = ts(1_000);
    diesel::update(agent_conversations.filter(conversation_id.eq("conv-1")))
        .set((
            summary.eq(Some("{not valid json")),
            last_modified_at.eq(legacy_ts),
        ))
        .execute(&mut conn)
        .expect("corrupt summary setup should succeed");

    let (_, backfills) =
        read_agent_conversation_metadata(&mut conn).expect("metadata read should succeed");
    assert_eq!(backfills.len(), 1);
    assert_eq!(
        backfills[0].previous_summary.as_deref(),
        Some("{not valid json"),
        "the backfill must carry the observed invalid value for its compare-and-set"
    );

    backfill_conversation_summaries(&mut conn, backfills).expect("backfill should succeed");

    // The invalid summary healed in place and history order is preserved.
    let healed: AgentConversationSummary = serde_json::from_str(
        summary_column(&mut conn, "conv-1")
            .as_deref()
            .expect("summary should be present after healing"),
    )
    .expect("healed summary should be valid JSON");
    assert_eq!(healed.initial_query, "Initial query");
    assert_eq!(last_modified_column(&mut conn, "conv-1"), legacy_ts);

    // Subsequent startups stay metadata-only.
    let (_, backfills) =
        read_agent_conversation_metadata(&mut conn).expect("metadata read should succeed");
    assert!(backfills.is_empty());
}

fn ts(secs_from_epoch: i64) -> NaiveDateTime {
    // 2026-01-01 baseline keeps failure messages readable.
    NaiveDate::from_ymd_opt(2026, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        + chrono::Duration::seconds(secs_from_epoch)
}

fn make_row(id: i32, conversation_id: &str, secs: i64) -> AgentConversationRecord {
    AgentConversationRecord {
        id,
        conversation_id: conversation_id.to_string(),
        conversation_data: r#"{"server_conversation_token":null}"#.to_string(),
        last_modified_at: ts(secs),
        summary: None,
    }
}

/// Row count ≤ limit ⇒ no eviction.
#[test]
fn prune_is_no_op_when_under_limit() {
    let rows = vec![
        make_row(1, "a", 100),
        make_row(2, "b", 200),
        make_row(3, "c", 300),
    ];
    assert!(select_conversations_to_evict(&rows, 3).is_empty());
    assert!(select_conversations_to_evict(&rows, 100).is_empty());
}

/// The oldest rows are evicted first.
#[test]
fn evicts_the_oldest_rows() {
    let rows: Vec<_> = (0_i32..10)
        .map(|i| make_row(i, &format!("s{i}"), 10 + i64::from(i)))
        .collect();
    let evicted = select_conversations_to_evict(&rows, 8);
    assert_eq!(evicted, vec!["s0".to_string(), "s1".to_string()]);
}

/// Rows written by older builds may still carry a `parent_conversation_id`; the key is ignored and
/// the row is treated like any other.
#[test]
fn legacy_parent_keys_are_ignored() {
    let mut rows = vec![
        make_row(1, "parent", 1),
        make_row(2, "child", 9_999),
        make_row(3, "other", 500),
    ];
    rows[1].conversation_data =
        r#"{"server_conversation_token":null,"parent_conversation_id":"parent"}"#.to_string();
    let evicted = select_conversations_to_evict(&rows, 2);
    assert_eq!(evicted, vec!["parent".to_string()]);
}

/// A row whose data does not parse is still an ordinary row.
#[test]
fn parse_failure_rows_are_ordinary() {
    let rows = vec![
        AgentConversationRecord {
            id: 1,
            conversation_id: "garbage".to_string(),
            conversation_data: "{not valid json".to_string(),
            last_modified_at: ts(50),
            summary: None,
        },
        make_row(2, "a", 100),
        make_row(3, "b", 200),
    ];
    assert_eq!(
        select_conversations_to_evict(&rows, 2),
        vec!["garbage".to_string()]
    );
}

/// The freshest row is retained even when the limit is zero.
#[test]
fn freshest_row_is_always_kept() {
    let rows = vec![make_row(1, "old", 1), make_row(2, "new", 2)];
    assert_eq!(
        select_conversations_to_evict(&rows, 0),
        vec!["old".to_string()]
    );
}

/// Same input twice produces the same output. Ties are broken by conversation id ASC.
#[test]
fn eviction_is_deterministic() {
    let rows = vec![
        make_row(1, "a", 100),
        make_row(2, "b", 100),
        make_row(3, "c", 100),
        make_row(4, "d", 100),
    ];
    let e1 = select_conversations_to_evict(&rows, 2);
    let e2 = select_conversations_to_evict(&rows, 2);
    assert_eq!(e1, e2);
    assert_eq!(e1, vec!["c".to_string(), "d".to_string()]);
}

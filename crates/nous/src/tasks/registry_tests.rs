#![expect(clippy::expect_used, reason = "test assertions")]
#![expect(
    clippy::indexing_slicing,
    reason = "test assertions on collections with previously verified length"
)]

use std::time::Duration;

use super::*;

fn make_registry() -> TaskRegistry {
    TaskRegistry::new(Duration::from_secs(1))
}

#[test]
fn register_and_get() {
    let reg = make_registry();
    let (id, _token) = reg
        .register(
            TaskType::Shell {
                command: "echo hello".into(),
            },
            "test shell".into(),
        )
        .expect("register");

    let snap = reg.get(id).expect("get");
    assert_eq!(snap.id, id);
    assert_eq!(snap.status, TaskStatus::Pending);
    assert_eq!(snap.description, "test shell");
}

#[test]
fn lifecycle_pending_to_running_to_completed() {
    let reg = make_registry();
    let (id, _token) = reg
        .register(
            TaskType::Agent {
                agent_id: "alice".into(),
                prompt: "research".into(),
            },
            "agent task".into(),
        )
        .expect("register");

    reg.update_status(id, TaskStatus::Running)
        .expect("to running");
    assert_eq!(reg.get(id).expect("get").status, TaskStatus::Running);

    reg.update_status(id, TaskStatus::Completed)
        .expect("to completed");
    let snap = reg.get(id).expect("get");
    assert_eq!(snap.status, TaskStatus::Completed);
    assert!(snap.completed_at.is_some());
}

#[test]
fn lifecycle_pending_to_running_to_failed() {
    let reg = make_registry();
    let (id, _token) = reg
        .register(
            TaskType::Shell {
                command: "false".into(),
            },
            "failing task".into(),
        )
        .expect("register");

    reg.update_status(id, TaskStatus::Running)
        .expect("to running");
    reg.update_status(id, TaskStatus::Failed)
        .expect("to failed");

    let snap = reg.get(id).expect("get");
    assert_eq!(snap.status, TaskStatus::Failed);
    assert!(snap.completed_at.is_some());
}

#[test]
fn terminal_to_running_is_invalid() {
    let reg = make_registry();
    let (id, _token) = reg
        .register(
            TaskType::Monitor {
                target: "health".into(),
            },
            "monitor".into(),
        )
        .expect("register");

    reg.update_status(id, TaskStatus::Running)
        .expect("to running");
    reg.update_status(id, TaskStatus::Completed)
        .expect("to completed");

    let result = reg.update_status(id, TaskStatus::Running);
    assert!(result.is_err());
}

#[test]
fn pending_to_completed_is_invalid() {
    let reg = make_registry();
    let (id, _token) = reg
        .register(
            TaskType::Workflow {
                name: "deploy".into(),
            },
            "workflow".into(),
        )
        .expect("register");

    // WHY: Can't skip Running to reach Completed.
    let result = reg.update_status(id, TaskStatus::Completed);
    assert!(result.is_err());
}

#[test]
fn pending_to_failed_is_valid() {
    let reg = make_registry();
    let (id, _token) = reg
        .register(
            TaskType::Shell {
                command: "bad-cmd".into(),
            },
            "fail fast".into(),
        )
        .expect("register");

    // WHY: Tasks can fail before they start (e.g. validation failure).
    reg.update_status(id, TaskStatus::Failed)
        .expect("to failed");
    assert_eq!(reg.get(id).expect("get").status, TaskStatus::Failed);
}

#[test]
fn kill_sets_status_and_cancels_token() {
    let reg = make_registry();
    let (id, token) = reg
        .register(
            TaskType::Agent {
                agent_id: "bob".into(),
                prompt: "long task".into(),
            },
            "killable".into(),
        )
        .expect("register");

    reg.update_status(id, TaskStatus::Running)
        .expect("to running");
    assert!(!token.is_cancelled());

    reg.kill(id).expect("kill");
    assert!(token.is_cancelled());
    assert_eq!(reg.get(id).expect("get").status, TaskStatus::Killed);
}

#[test]
fn kill_terminal_task_is_noop() {
    let reg = make_registry();
    let (id, _token) = reg
        .register(
            TaskType::Shell {
                command: "done".into(),
            },
            "done task".into(),
        )
        .expect("register");

    reg.update_status(id, TaskStatus::Running)
        .expect("to running");
    reg.update_status(id, TaskStatus::Completed)
        .expect("to completed");

    // WHY: Killing an already-completed task should be silently ignored.
    reg.kill(id).expect("kill noop");
    assert_eq!(reg.get(id).expect("get").status, TaskStatus::Completed);
}

#[test]
fn get_nonexistent_returns_not_found() {
    let reg = make_registry();
    let fake_id = TaskId::new();
    let result = reg.get(fake_id);
    assert!(result.is_err());
}

#[test]
fn list_all_tasks() {
    let reg = make_registry();
    reg.register(
        TaskType::Shell {
            command: "a".into(),
        },
        "task a".into(),
    )
    .expect("register a");
    reg.register(
        TaskType::Shell {
            command: "b".into(),
        },
        "task b".into(),
    )
    .expect("register b");

    let all = reg.list(None).expect("list");
    assert_eq!(all.len(), 2);
}

#[test]
fn list_with_status_filter() {
    let reg = make_registry();

    let (id_a, _) = reg
        .register(
            TaskType::Shell {
                command: "a".into(),
            },
            "task a".into(),
        )
        .expect("register a");
    let (_id_b, _) = reg
        .register(
            TaskType::Shell {
                command: "b".into(),
            },
            "task b".into(),
        )
        .expect("register b");

    reg.update_status(id_a, TaskStatus::Running)
        .expect("to running");

    let running = reg.list(Some(TaskStatus::Running)).expect("list running");
    assert_eq!(running.len(), 1);
    assert_eq!(running[0].id, id_a);

    let pending = reg.list(Some(TaskStatus::Pending)).expect("list pending");
    assert_eq!(pending.len(), 1);
}

#[test]
fn record_tool_call_updates_activity() {
    let reg = make_registry();
    let (id, _) = reg
        .register(
            TaskType::Agent {
                agent_id: "alice".into(),
                prompt: "work".into(),
            },
            "agent".into(),
        )
        .expect("register");

    reg.record_tool_call(
        id,
        ToolCallSummary {
            tool_name: "read_file".into(),
            elapsed: jiff::SignedDuration::from_millis(150),
        },
    )
    .expect("record");

    let snap = reg.get(id).expect("get");
    assert_eq!(snap.recent_activity.len(), 1);
    assert_eq!(snap.recent_activity[0].tool_name, "read_file");
}

#[test]
fn record_error_sets_snapshot() {
    let reg = make_registry();
    let (id, _) = reg
        .register(
            TaskType::Shell {
                command: "oops".into(),
            },
            "erroring".into(),
        )
        .expect("register");

    reg.record_error(id, "something went wrong".into())
        .expect("record error");

    let snap = reg.get(id).expect("get");
    assert_eq!(snap.error_snapshot.as_deref(), Some("something went wrong"));
}

#[tokio::test]
async fn progress_subscribe_receives_events() {
    let reg = make_registry();
    let (id, _) = reg
        .register(
            TaskType::Consolidation { sessions_count: 5 },
            "consolidate".into(),
        )
        .expect("register");

    let mut rx = reg.subscribe(id).expect("subscribe");

    reg.update_status(id, TaskStatus::Running)
        .expect("to running");

    let event = rx.recv().await.expect("recv");
    match event {
        ProgressEvent::StatusChanged { from, to } => {
            assert_eq!(from, TaskStatus::Pending);
            assert_eq!(to, TaskStatus::Running);
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[tokio::test]
async fn progress_subscribe_receives_tool_activity() {
    let reg = make_registry();
    let (id, _) = reg
        .register(
            TaskType::Agent {
                agent_id: "alice".into(),
                prompt: "work".into(),
            },
            "agent".into(),
        )
        .expect("register");

    let mut rx = reg.subscribe(id).expect("subscribe");

    reg.record_tool_call(
        id,
        ToolCallSummary {
            tool_name: "search".into(),
            elapsed: jiff::SignedDuration::from_millis(200),
        },
    )
    .expect("record");

    let event = rx.recv().await.expect("recv");
    match event {
        ProgressEvent::ToolActivity(summary) => {
            assert_eq!(summary.tool_name, "search");
        }
        other => panic!("unexpected event: {other:?}"),
    }
}

#[test]
fn concurrent_register_from_multiple_threads() {
    let reg = make_registry();
    let reg_clone = reg.clone();

    let handles: Vec<_> = (0..10)
        .map(|i| {
            let r = reg_clone.clone();
            std::thread::spawn(move || {
                r.register(
                    TaskType::Shell {
                        command: format!("cmd_{i}"),
                    },
                    format!("task {i}"),
                )
                .expect("register");
            })
        })
        .collect();

    for h in handles {
        h.join().expect("thread join");
    }

    assert_eq!(reg.len().expect("len"), 10);
}

#[test]
fn gc_sweep_evicts_stale_tasks() {
    // WHY: Use a zero deadline so tasks are immediately eligible.
    let reg = TaskRegistry::new(Duration::from_secs(0));

    let (id, _) = reg
        .register(
            TaskType::Shell {
                command: "done".into(),
            },
            "stale".into(),
        )
        .expect("register");

    reg.update_status(id, TaskStatus::Running)
        .expect("to running");
    reg.update_status(id, TaskStatus::Completed)
        .expect("to completed");

    let evicted = reg.gc_sweep().expect("gc sweep");
    assert_eq!(evicted.len(), 1);
    assert_eq!(evicted[0].0, id);

    // WHY: After GC, the task should no longer be in the registry.
    assert!(reg.get(id).is_err());
}

#[test]
fn gc_sweep_retains_non_terminal_tasks() {
    let reg = TaskRegistry::new(Duration::from_secs(0));

    let (id, _) = reg
        .register(
            TaskType::Shell {
                command: "running".into(),
            },
            "active".into(),
        )
        .expect("register");

    reg.update_status(id, TaskStatus::Running)
        .expect("to running");

    let evicted = reg.gc_sweep().expect("gc sweep");
    assert!(evicted.is_empty());
    assert!(reg.get(id).is_ok());
}

#[test]
fn len_and_is_empty() {
    let reg = make_registry();
    assert!(reg.is_empty().expect("is_empty"));
    assert_eq!(reg.len().expect("len"), 0);

    reg.register(
        TaskType::Monitor {
            target: "mcp".into(),
        },
        "monitor".into(),
    )
    .expect("register");

    assert!(!reg.is_empty().expect("is_empty"));
    assert_eq!(reg.len().expect("len"), 1);
}
